//! Codex's server→client requests: approvals and questions become approval
//! items; anything chat can't serve is declined on the spot.

use serde_json::{json, Map, Value};

use super::items::{file_change_call, strip_shell};
use super::CodexTranscript;
use crate::claude_transcript::{clip_value, str_at, ApprovalDecision, TranscriptItem};

/// An approval item waiting for an answer.
#[derive(Debug)]
pub(super) struct Pending {
    pub item: usize,
    /// The JSON-RPC id to answer.
    rpc_id: Value,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    /// `availableDecisions` as offered (absent on older servers: all of them).
    Command {
        offered: Option<Vec<Value>>,
    },
    FileChange,
    Permissions {
        requested: Value,
    },
    /// Question text → question id, in order.
    Questions {
        ids: Vec<(String, String)>,
    },
}

/// The approval item id for a request id.
fn approval_id(rpc_id: &Value) -> String {
    match rpc_id {
        Value::String(s) => format!("request:{s}"),
        other => format!("request:{other}"),
    }
}

fn response(rpc_id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": rpc_id, "result": result})
}

impl CodexTranscript {
    /// A request from codex; `Some` is the response to send now.
    pub(super) fn apply_request(
        &mut self,
        rpc_id: &Value,
        method: &str,
        params: &Value,
        changed: &mut Vec<usize>,
    ) -> Option<Value> {
        let reason = str_at(params, "reason").map(String::from);
        let (tool, input, kind, can_always_allow, blocked_path) = match method {
            "item/commandExecution/requestApproval" => {
                let offered = params
                    .get("availableDecisions")
                    .and_then(Value::as_array)
                    .cloned();
                // Not `acceptWithExecpolicyAmendment`: that saves a lasting rule,
                // and the button promises this session only.
                let always = offered
                    .as_ref()
                    .is_none_or(|o| o.iter().any(|d| d == "acceptForSession"));
                let input = json!({
                    "command": strip_shell(str_at(params, "command").unwrap_or_default()),
                    "cwd": str_at(params, "cwd").unwrap_or_default(),
                });
                (
                    "Bash".to_string(),
                    input,
                    Kind::Command { offered },
                    always,
                    None,
                )
            }
            "item/fileChange/requestApproval" => {
                let changes = str_at(params, "itemId")
                    .and_then(|id| self.file_changes.get(id))
                    .cloned()
                    .unwrap_or_default();
                let (tool, mut input) = match changes.first() {
                    Some(first) => file_change_call(first),
                    None => ("Edit", json!({})),
                };
                if changes.len() > 1 {
                    let paths: Vec<&str> =
                        changes.iter().filter_map(|c| str_at(c, "path")).collect();
                    input["files"] = json!(paths);
                }
                let root = str_at(params, "grantRoot").map(String::from);
                (tool.to_string(), input, Kind::FileChange, true, root)
            }
            "item/permissions/requestApproval" => {
                let requested = params.get("permissions").cloned().unwrap_or(json!({}));
                let input = json!({
                    "permissions": requested,
                    "cwd": str_at(params, "cwd").unwrap_or_default(),
                });
                let kind = Kind::Permissions { requested };
                ("Permissions".to_string(), input, kind, true, None)
            }
            "item/tool/requestUserInput" => {
                let raw = params.get("questions").and_then(Value::as_array);
                let raw = raw.map(Vec::as_slice).unwrap_or_default();
                let ids = raw
                    .iter()
                    .filter_map(|q| Some((str_at(q, "question")?.into(), str_at(q, "id")?.into())))
                    .collect();
                let questions: Vec<Value> = raw
                    .iter()
                    .map(|q| {
                        let options: Vec<Value> = q
                            .get("options")
                            .and_then(Value::as_array)
                            .map(|o| {
                                o.iter()
                                    .map(|o| json!({"label": o.get("label"), "description": o.get("description")}))
                                    .collect()
                            })
                            .unwrap_or_default();
                        json!({
                            "question": q.get("question"),
                            "header": str_at(q, "header").unwrap_or_default(),
                            "options": options,
                            "multiSelect": false,
                        })
                    })
                    .collect();
                let input = json!({ "questions": questions });
                (
                    "AskUserQuestion".to_string(),
                    input,
                    Kind::Questions { ids },
                    false,
                    None,
                )
            }
            "mcpServer/elicitation/request" => {
                return Some(response(
                    rpc_id,
                    json!({"action": "decline", "content": null, "_meta": null}),
                ));
            }
            _ => {
                if self.unknown_seen.insert(method.to_string()) {
                    log::warn!("codex sent a request chat can't serve: {method}");
                }
                return Some(json!({
                    "jsonrpc": "2.0",
                    "id": rpc_id,
                    "error": {
                        "code": -32601,
                        "message": format!("Workbench chat doesn't support `{method}` requests yet."),
                    },
                }));
            }
        };
        let id = approval_id(rpc_id);
        let item = TranscriptItem::Approval {
            id: id.clone(),
            tool,
            input: clip_value(&input),
            description: reason,
            blocked_path,
            can_always_allow,
            expired: false,
            decision: None,
            answers: None,
        };
        let i = self.upsert(item, changed);
        self.approvals.insert(
            id,
            Pending {
                item: i,
                rpc_id: rpc_id.clone(),
                kind,
            },
        );
        None
    }

    /// Record the answer to an approval and build codex's response. `None` if
    /// it's unknown or already answered (another device got there first).
    ///
    /// `answers` (question text → chosen label, or the person's own words)
    /// answers an `AskUserQuestion`.
    pub fn resolve_approval(
        &mut self,
        approval_id: &str,
        decision: ApprovalDecision,
        answers: Option<&Map<String, Value>>,
    ) -> Option<(usize, Value)> {
        let pending = self.approvals.remove(approval_id)?;
        let deny = decision == ApprovalDecision::Deny;
        let answers: Option<Map<String, Value>> = answers.filter(|_| !deny).map(|a| {
            a.iter()
                .filter(|(_, v)| v.is_string())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        });
        if let TranscriptItem::Approval {
            decision: d,
            answers: a,
            ..
        } = &mut self.items[pending.item]
        {
            *d = Some(decision);
            *a = answers.clone().map(Value::Object);
        }
        let result = match &pending.kind {
            Kind::Command { offered } => {
                let has = |d: &str| offered.as_ref().is_none_or(|o| o.iter().any(|v| v == d));
                let choice = match decision {
                    ApprovalDecision::AlwaysAllow if has("acceptForSession") => {
                        json!("acceptForSession")
                    }
                    ApprovalDecision::Allow | ApprovalDecision::AlwaysAllow => json!("accept"),
                    ApprovalDecision::Deny if has("decline") => json!("decline"),
                    ApprovalDecision::Deny => json!("cancel"),
                };
                json!({ "decision": choice })
            }
            Kind::FileChange => json!({"decision": match decision {
                ApprovalDecision::Allow => "accept",
                ApprovalDecision::AlwaysAllow => "acceptForSession",
                ApprovalDecision::Deny => "decline",
            }}),
            Kind::Permissions { requested } => match decision {
                ApprovalDecision::Deny => json!({"permissions": {}, "scope": "turn"}),
                _ => json!({
                    "permissions": granted(requested),
                    "scope": if decision == ApprovalDecision::AlwaysAllow { "session" } else { "turn" },
                }),
            },
            Kind::Questions { ids } => {
                let answers = answers.unwrap_or_default();
                let picked: Map<String, Value> = ids
                    .iter()
                    .filter_map(|(question, id)| {
                        let text = answers.get(question)?.as_str()?;
                        Some((id.clone(), json!({ "answers": [text] })))
                    })
                    .collect();
                json!({ "answers": picked })
            }
        };
        Some((pending.item, response(&pending.rpc_id, result)))
    }

    /// Withdraw every open approval (an interrupt): marks them expired and
    /// returns the `cancel` responses codex is waiting for.
    pub fn cancel_approvals(&mut self) -> (Vec<usize>, Vec<Value>) {
        let mut changed = Vec::new();
        let mut replies = Vec::new();
        for (_, pending) in std::mem::take(&mut self.approvals) {
            let result = match pending.kind {
                Kind::Command { .. } | Kind::FileChange => json!({"decision": "cancel"}),
                Kind::Permissions { .. } => json!({"permissions": {}, "scope": "turn"}),
                Kind::Questions { .. } => json!({"answers": {}}),
            };
            replies.push(response(&pending.rpc_id, result));
            self.mark_expired(pending.item, &mut changed);
        }
        changed.sort_unstable();
        (changed, replies)
    }

    /// Codex settled a request itself (`serverRequest/resolved`).
    pub(super) fn expire_request(&mut self, rpc_id: &Value, changed: &mut Vec<usize>) {
        if let Some(pending) = self.approvals.remove(&approval_id(rpc_id)) {
            self.mark_expired(pending.item, changed);
        }
    }

    pub(super) fn expire_all(&mut self, changed: &mut Vec<usize>) {
        for (_, pending) in std::mem::take(&mut self.approvals) {
            self.mark_expired(pending.item, changed);
        }
    }

    fn mark_expired(&mut self, i: usize, changed: &mut Vec<usize>) {
        if let TranscriptItem::Approval { expired, .. } = &mut self.items[i] {
            *expired = true;
            changed.push(i);
        }
    }
}

/// The requested permissions as a grant: the same profile without its null parts.
fn granted(requested: &Value) -> Value {
    let kept: Map<String, Value> = requested
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    Value::Object(kept)
}
