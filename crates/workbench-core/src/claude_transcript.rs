//! A Claude Code session as chat items.
//!
//! Two sources feed the same [`Transcript`]: the session JSONL the CLI writes
//! (`~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`, one content block
//! per line) for history, and the `--output-format stream-json` events of a
//! live `claude -p` process. Both shapes are internal to the CLI, so unknown
//! lines are ignored rather than treated as errors.
//!
//! Tool results update the call they answer; with `--include-partial-messages`
//! text streams into an item before the final block replaces it in place; a
//! `can_use_tool` control request becomes an approval item whose answer
//! [`Transcript::resolve_approval`] turns back into a control response.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::BufRead;
use std::path::Path;

use serde_json::{json, Value};

mod items;
mod parse;
mod protocol;

pub use items::{
    ApprovalDecision, ModelOption, RateLimitInfo, RetryInfo, TaskInfo, ToolStatus, TranscriptItem,
    TranscriptMeta,
};

use parse::{clip, clip_patch, clip_value, str_at, tool_output_text, user_visible_text, UserText};
pub use parse::{find_transcript, is_uuid};

/// What one [`Transcript::apply`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    /// Indices into [`Transcript::items`] that were added or updated.
    pub items: Vec<usize>,
    pub meta: bool,
    /// A `control_response` the host must write back (unsupported requests are
    /// answered with an error — unanswered, the CLI would wait forever).
    pub reply: Option<Value>,
    /// The session continued under a new id (`/clear`); items were reset.
    pub new_session_id: Option<String>,
    /// First sighting of a message kind not in [`protocol`]'s inventory.
    pub unknown_kind: Option<String>,
}

/// What an approval needs to be answered: the input to echo back and the
/// CLI's suggested rules for "always allow".
#[derive(Debug)]
struct PendingApproval {
    item: usize,
    input: Value,
    suggestions: Value,
}

#[derive(Debug, Default)]
pub struct Transcript {
    items: Vec<TranscriptItem>,
    index: HashMap<String, usize>,
    meta: TranscriptMeta,
    /// Message id of the assistant message currently streaming.
    streaming_message: Option<String>,
    /// Streamed text/thinking items, in block order, awaiting their final block.
    stream_slots: HashMap<String, VecDeque<usize>>,
    approvals: HashMap<String, PendingApproval>,
    unknown_seen: std::collections::HashSet<String>,
    /// Whole outputs of tools whose item only carries a preview.
    full_outputs: HashMap<String, String>,
}

/// Largest tool output kept whole for "show full output".
const MAX_FULL_OUTPUT_BYTES: usize = 1024 * 1024;

impl Transcript {
    /// History from a session JSONL. A missing or unreadable file is an empty
    /// transcript: a brand-new session has no file yet.
    pub fn load(path: &Path) -> Self {
        let mut t = Self::default();
        if let Ok(file) = fs::File::open(path) {
            for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
                t.apply_line(&line);
            }
        }
        // History never has a turn in flight: the process that wrote it is gone.
        t.meta.busy = false;
        t
    }

    pub fn items(&self) -> &[TranscriptItem] {
        &self.items
    }

    pub fn meta(&self) -> &TranscriptMeta {
        &self.meta
    }

    pub fn apply_line(&mut self, line: &str) -> Applied {
        match serde_json::from_str::<Value>(line) {
            Ok(obj) => self.apply(&obj),
            Err(_) => Applied::default(),
        }
    }

    pub fn apply(&mut self, obj: &Value) -> Applied {
        // Subagent turns belong to their own transcript; the Task tool card
        // already stands for them here.
        if obj.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || obj.get("parent_tool_use_id").is_some_and(|v| !v.is_null())
        {
            return Applied::default();
        }
        let before = self.meta.clone();
        let mut changed = Vec::new();
        let mut applied = Applied::default();
        let kind = protocol::kind(obj);
        if !protocol::is_known(&kind) && self.unknown_seen.insert(kind.clone()) {
            applied.unknown_kind = Some(kind);
        }
        match str_at(obj, "type") {
            Some("user") => self.apply_user(obj, &mut changed),
            Some("assistant") => self.apply_assistant(obj, &mut changed),
            Some("stream_event") => self.apply_stream_event(obj, &mut changed),
            Some("control_request") => {
                applied.reply = self.apply_control_request(obj, &mut changed)
            }
            Some("control_cancel_request") => self.expire_approval(obj, &mut changed),
            Some("control_response") => self.apply_control_response(obj),
            Some("conversation_reset") => {
                let next = str_at(obj, "new_conversation_id").map(String::from);
                self.reset();
                applied.new_session_id = next;
            }
            Some("attachment") => self.apply_queued_prompt(obj, &mut changed),
            Some("result") => self.apply_result(obj, &mut changed),
            Some("rate_limit_event") => self.apply_rate_limit(obj),
            Some("ai-title") => self.meta.title = str_at(obj, "aiTitle").map(String::from),
            Some("permission-mode") => {
                self.meta.permission_mode = str_at(obj, "permissionMode").map(String::from)
            }
            Some("system") => match str_at(obj, "subtype") {
                Some("init") => {
                    self.meta.model = str_at(obj, "model").map(String::from);
                    self.meta.permission_mode = str_at(obj, "permissionMode").map(String::from);
                }
                Some("status") if str_at(obj, "status") == Some("requesting") => {
                    self.meta.busy = true
                }
                Some("turn_duration") => self.meta.busy = false,
                // Output of a slash command run in chat (`/cost`, `/context`).
                Some("local_command_output") => {
                    if let Some(text) = str_at(obj, "content").filter(|t| !t.trim().is_empty()) {
                        let id = str_at(obj, "uuid").unwrap_or_default().to_string();
                        self.upsert(
                            TranscriptItem::Notice {
                                id,
                                text: clip(text),
                            },
                            &mut changed,
                        );
                    }
                }
                Some("task_started" | "task_progress" | "task_updated" | "task_notification") => {
                    self.apply_task(obj)
                }
                Some("background_tasks_changed") => self.apply_background_tasks(obj),
                Some("api_retry") => {
                    let n = |k| obj.get(k).and_then(Value::as_u64).unwrap_or(0);
                    self.meta.retry = Some(RetryInfo {
                        attempt: n("attempt"),
                        max_retries: n("max_retries"),
                        retry_delay_ms: n("retry_delay_ms"),
                        error: str_at(obj, "error").map(String::from),
                    });
                }
                Some("compact_boundary") => {
                    let id = str_at(obj, "uuid").unwrap_or("compact").to_string();
                    let text = "Conversation compacted".to_string();
                    self.upsert(TranscriptItem::Notice { id, text }, &mut changed);
                }
                _ => {}
            },
            _ => {}
        }
        applied.items = changed;
        applied.meta = self.meta != before;
        applied
    }

    fn apply_rate_limit(&mut self, obj: &Value) {
        let Some(info) = obj.get("rate_limit_info") else {
            return;
        };
        let Some(status) = str_at(info, "status") else {
            return;
        };
        self.meta.rate_limit = Some(RateLimitInfo {
            status: status.to_string(),
            resets_at: info.get("resetsAt").and_then(Value::as_u64),
            kind: str_at(info, "rateLimitType").map(String::from),
            utilization: info.get("utilization").and_then(Value::as_f64),
        });
    }

    /// The whole output of a tool whose item carries a preview.
    pub fn full_output(&self, tool_id: &str) -> Option<&str> {
        self.full_outputs.get(tool_id).map(String::as_str)
    }

    fn task_mut(&mut self, id: &str) -> &mut TaskInfo {
        let tasks = &mut self.meta.tasks;
        let i = match tasks.iter().position(|t| t.id == id) {
            Some(i) => i,
            None => {
                tasks.push(TaskInfo {
                    id: id.to_string(),
                    kind: "agent".into(),
                    status: "running".into(),
                    ..TaskInfo::default()
                });
                tasks.len() - 1
            }
        };
        &mut tasks[i]
    }

    fn apply_task(&mut self, obj: &Value) {
        let Some(id) = str_at(obj, "task_id") else {
            return;
        };
        let progress = str_at(obj, "subtype") == Some("task_progress");
        let task = self.task_mut(id);
        let text = |k| str_at(obj, k).filter(|s| !s.is_empty()).map(String::from);
        // A progress event's description is the current step, not the task's name.
        match text("description") {
            Some(d) if progress => task.activity = Some(d),
            Some(d) => task.description = d,
            None => {}
        }
        if let Some(t) = text("tool_use_id") {
            task.tool_use_id = Some(t);
        }
        if let Some(t) = text("subagent_type") {
            task.subagent_type = Some(t);
        }
        if let Some(t) = text("task_type") {
            task.kind = if t.contains("agent") {
                "agent".into()
            } else {
                t
            };
        }
        if let Some(b) = obj.get("is_backgrounded").and_then(Value::as_bool) {
            task.background = b;
        }
        if let Some(usage) = obj.get("usage") {
            let n = |k| usage.get(k).and_then(Value::as_u64);
            task.tool_uses = n("tool_uses").unwrap_or(task.tool_uses);
            task.tokens = n("total_tokens").unwrap_or(task.tokens);
            task.duration_ms = n("duration_ms").unwrap_or(task.duration_ms);
        }
        if let Some(t) = text("last_tool_name") {
            task.last_tool = Some(t);
        }
        if let Some(t) = text("summary") {
            task.summary = Some(clip(&t));
        }
        if let Some(status) = text("status") {
            task.status = status; // task_notification: completed | failed | stopped
            task.activity = None;
        }
        if let Some(patch) = obj.get("patch") {
            if let Some(status) = str_at(patch, "status") {
                task.status = status.to_string();
            }
            if let Some(d) = str_at(patch, "description") {
                task.description = d.to_string();
            }
            if let Some(e) = str_at(patch, "error") {
                task.summary = Some(clip(e));
            }
            if let Some(b) = patch.get("is_backgrounded").and_then(Value::as_bool) {
                task.background = b;
            }
        }
    }

    /// The CLI's list of live background jobs: add any not seen starting.
    fn apply_background_tasks(&mut self, obj: &Value) {
        let Some(list) = obj.get("tasks").and_then(Value::as_array) else {
            return;
        };
        for entry in list {
            let Some(id) = str_at(entry, "task_id") else {
                continue;
            };
            let task_type = str_at(entry, "task_type")
                .unwrap_or("background")
                .to_string();
            let description = str_at(entry, "description").unwrap_or_default().to_string();
            let task = self.task_mut(id);
            task.background = true;
            task.kind = if task_type.contains("agent") {
                "agent".into()
            } else {
                task_type
            };
            if task.description.is_empty() {
                task.description = description;
            }
        }
    }

    /// `/clear` and friends: the conversation starts over under a new id.
    fn reset(&mut self) {
        let meta = TranscriptMeta {
            busy: false,
            title: None,
            context_tokens: None,
            tasks: Vec::new(),
            ..self.meta.clone()
        };
        *self = Self {
            meta,
            unknown_seen: std::mem::take(&mut self.unknown_seen),
            ..Self::default()
        };
    }

    fn expire_approval(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        let Some(pending) = str_at(obj, "request_id").and_then(|id| self.approvals.remove(id))
        else {
            return;
        };
        if let TranscriptItem::Approval { expired, .. } = &mut self.items[pending.item] {
            *expired = true;
        }
        changed.push(pending.item);
    }

    /// Mark a prompt as sent before the CLI echoes it, so the chat shows the
    /// turn starting at once.
    pub fn set_busy(&mut self) {
        self.meta.busy = true;
    }

    /// The mode just requested with `set_permission_mode`.
    pub fn set_permission_mode(&mut self, mode: &str) {
        self.meta.permission_mode = Some(mode.to_string());
    }

    /// The model just requested with `set_model`.
    pub fn set_model_choice(&mut self, value: &str) {
        self.meta.model_choice = Some(value.to_string());
        let resolved = self
            .meta
            .models
            .iter()
            .find(|m| m.value == value)
            .and_then(|m| m.resolved_model.clone());
        if let Some(model) = resolved {
            self.meta.model = Some(model);
        }
    }

    /// The effort level just requested.
    pub fn set_effort(&mut self, level: &str) {
        self.meta.effort = Some(level.to_string());
    }

    /// Replies to the host's own requests; the `initialize` reply lists the
    /// models the session can switch to.
    fn apply_control_response(&mut self, obj: &Value) {
        let Some(models) = obj
            .pointer("/response/response/models")
            .and_then(Value::as_array)
        else {
            return;
        };
        self.meta.models = models
            .iter()
            .filter_map(|m| {
                Some(ModelOption {
                    value: str_at(m, "value")?.to_string(),
                    display_name: str_at(m, "displayName").unwrap_or_default().to_string(),
                    description: str_at(m, "description").unwrap_or_default().to_string(),
                    resolved_model: str_at(m, "resolvedModel").map(String::from),
                    effort_levels: m
                        .get("supportedEffortLevels")
                        .and_then(Value::as_array)
                        .map(|l| {
                            l.iter()
                                .filter_map(Value::as_str)
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect();
    }

    /// Record the answer to an approval and build the `control_response` for
    /// the CLI. `None` if the request is unknown or already answered.
    ///
    /// `answers` (question text → chosen label, or the person's own words) is
    /// how an `AskUserQuestion` call is answered: the tool reads them from its
    /// `updatedInput`. Only string values are passed on.
    pub fn resolve_approval(
        &mut self,
        request_id: &str,
        decision: ApprovalDecision,
        answers: Option<&serde_json::Map<String, Value>>,
    ) -> Option<(usize, Value)> {
        let pending = self.approvals.remove(request_id)?;
        let answers: Option<Value> = answers.map(|a| {
            a.iter()
                .filter(|(_, v)| v.is_string())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<_, _>>()
                .into()
        });
        let mut input = pending.input;
        if let (Some(answers), Some(obj)) = (&answers, input.as_object_mut()) {
            if decision != ApprovalDecision::Deny {
                obj.insert("answers".into(), answers.clone());
            }
        }
        if let TranscriptItem::Approval {
            decision: d,
            answers: a,
            ..
        } = &mut self.items[pending.item]
        {
            *d = Some(decision);
            if decision != ApprovalDecision::Deny {
                *a = answers;
            }
        }
        let response = match decision {
            ApprovalDecision::Deny => json!({
                "behavior": "deny",
                "message": "The user declined this in Workbench.",
            }),
            ApprovalDecision::Allow => json!({"behavior": "allow", "updatedInput": input}),
            ApprovalDecision::AlwaysAllow => json!({
                "behavior": "allow",
                "updatedInput": input,
                "updatedPermissions": pending.suggestions,
            }),
        };
        Some((
            pending.item,
            json!({
                "type": "control_response",
                "response": {"subtype": "success", "request_id": request_id, "response": response},
            }),
        ))
    }

    /// Request ids still waiting for an answer.
    pub fn pending_approval_ids(&self) -> Vec<String> {
        self.approvals.keys().cloned().collect()
    }

    /// Permission prompts become approval items; anything else the CLI asks of
    /// its host (MCP elicitation, hook callbacks, dialogs) gets an error reply.
    fn apply_control_request(&mut self, obj: &Value, changed: &mut Vec<usize>) -> Option<Value> {
        let request_id = str_at(obj, "request_id")?;
        let req = obj.get("request")?;
        if str_at(req, "subtype") != Some("can_use_tool") {
            let subtype = str_at(req, "subtype").unwrap_or("unknown");
            return Some(json!({
                "type": "control_response",
                "response": {
                    "subtype": "error",
                    "request_id": request_id,
                    "error": format!("Workbench chat doesn't support `{subtype}` requests yet."),
                },
            }));
        }
        let input = req.get("input").cloned().unwrap_or(Value::Null);
        let suggestions = req
            .get("permission_suggestions")
            .cloned()
            .unwrap_or(Value::Null);
        let can_always_allow = suggestions.as_array().is_some_and(|s| !s.is_empty());
        self.upsert(
            TranscriptItem::Approval {
                id: request_id.to_string(),
                tool: str_at(req, "tool_name").unwrap_or("tool").to_string(),
                input: clip_value(&input),
                description: str_at(req, "description").map(String::from),
                blocked_path: str_at(req, "blocked_path").map(String::from),
                can_always_allow,
                expired: false,
                decision: None,
                answers: None,
            },
            changed,
        );
        let item = self.index[request_id];
        self.approvals.insert(
            request_id.to_string(),
            PendingApproval {
                item,
                input,
                suggestions,
            },
        );
        None
    }

    fn apply_result(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        self.meta.busy = false;
        self.meta.retry = None;
        self.streaming_message = None;
        if obj.get("is_error").and_then(Value::as_bool) == Some(true) {
            let text = str_at(obj, "result")
                .or_else(|| str_at(obj, "subtype"))
                .unwrap_or("The turn failed.");
            let id = str_at(obj, "uuid").unwrap_or("result").to_string();
            self.upsert(
                TranscriptItem::Notice {
                    id,
                    text: clip(text),
                },
                changed,
            );
        }
    }

    fn apply_user(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        if obj.get("isMeta").and_then(Value::as_bool) == Some(true)
            || obj.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        let id = str_at(obj, "uuid").unwrap_or_default().to_string();
        // JSONL spells it toolUseResult; stream-json, tool_use_result.
        let result = obj
            .get("toolUseResult")
            .or_else(|| obj.get("tool_use_result"));
        let content = obj.get("message").and_then(|m| m.get("content"));
        let mut images = 0;
        let text = match content {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match str_at(block, "type") {
                        Some("tool_result") => self.apply_tool_result(block, result, changed),
                        Some("image") => images += 1,
                        _ => {}
                    }
                }
                let texts: Vec<&str> = blocks
                    .iter()
                    .filter(|b| str_at(b, "type") == Some("text"))
                    .filter_map(|b| str_at(b, "text"))
                    .collect();
                (!texts.is_empty() || images > 0).then(|| texts.join("\n"))
            }
            _ => None,
        };
        let Some(text) = text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty() || images > 0)
        else {
            return;
        };
        if text.starts_with("[Request interrupted") {
            self.meta.busy = false;
            self.upsert(
                TranscriptItem::Notice {
                    id,
                    text: "Interrupted".into(),
                },
                changed,
            );
            return;
        }
        let text = match if text.is_empty() {
            Some(UserText::Prompt(String::new()))
        } else {
            user_visible_text(text)
        } {
            Some(UserText::Prompt(text)) => {
                self.meta.busy = true;
                text
            }
            Some(UserText::Command(text)) => text,
            None => return,
        };
        let timestamp = str_at(obj, "timestamp").unwrap_or_default().to_string();
        self.upsert(
            TranscriptItem::User {
                id,
                text,
                timestamp,
                images,
            },
            changed,
        );
    }

    /// A prompt typed while a turn was running is recorded as an attachment
    /// when the CLI folds it into that turn.
    fn apply_queued_prompt(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        let Some(att) = obj.get("attachment") else {
            return;
        };
        if str_at(att, "type") != Some("queued_command")
            || str_at(att, "commandMode") != Some("prompt")
        {
            return;
        }
        let Some(text) = str_at(att, "prompt")
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            return;
        };
        let item = TranscriptItem::User {
            id: str_at(obj, "uuid").unwrap_or_default().to_string(),
            text: text.to_string(),
            timestamp: str_at(att, "timestamp").unwrap_or_default().to_string(),
            images: 0,
        };
        self.upsert(item, changed);
    }

    fn apply_tool_result(
        &mut self,
        block: &Value,
        result: Option<&Value>,
        changed: &mut Vec<usize>,
    ) {
        let Some(tool_id) = str_at(block, "tool_use_id") else {
            return;
        };
        let Some(&i) = self.index.get(tool_id) else {
            return;
        };
        let TranscriptItem::Tool {
            status,
            output,
            full_output_bytes,
            patch,
            ..
        } = &mut self.items[i]
        else {
            return;
        };
        let is_error = block.get("is_error").and_then(Value::as_bool) == Some(true);
        *status = if is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Ok
        };
        let text = tool_output_text(block.get("content"));
        *output = text.as_deref().map(clip);
        *full_output_bytes = None;
        if let Some(text) = text.filter(|t| t.len() > parse::MAX_TEXT_BYTES) {
            *full_output_bytes = Some(text.len());
            let kept = crate::text::truncate_bytes(&text, MAX_FULL_OUTPUT_BYTES).to_string();
            self.full_outputs.insert(tool_id.to_string(), kept);
        }
        *patch = result
            .and_then(|r| r.get("structuredPatch"))
            .and_then(clip_patch);
        changed.push(i);
    }

    fn apply_stream_event(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        let Some(event) = obj.get("event") else {
            return;
        };
        match str_at(event, "type") {
            Some("message_start") => {
                self.meta.busy = true;
                self.meta.retry = None;
                let message = event.get("message");
                self.streaming_message = message.and_then(|m| str_at(m, "id")).map(String::from);
                if let Some(model) = message.and_then(|m| str_at(m, "model")) {
                    self.meta.model = Some(model.to_string());
                }
            }
            Some("content_block_start") => {
                let (Some(msg), Some(index), Some(block)) = (
                    self.streaming_message.clone(),
                    event.get("index").and_then(Value::as_u64),
                    event.get("content_block"),
                ) else {
                    return;
                };
                let id = format!("{msg}:{index}");
                let item = match str_at(block, "type") {
                    Some("text") => TranscriptItem::Text {
                        id,
                        text: String::new(),
                    },
                    Some("thinking") => TranscriptItem::Thinking {
                        id,
                        text: String::new(),
                    },
                    // Shown as running while its input streams; the final
                    // assistant block fills the input in.
                    Some("tool_use") => TranscriptItem::Tool {
                        id: str_at(block, "id").unwrap_or(&id).to_string(),
                        name: str_at(block, "name").unwrap_or("tool").to_string(),
                        input: Value::Null,
                        status: ToolStatus::Running,
                        output: None,
                        full_output_bytes: None,
                        patch: None,
                    },
                    _ => return,
                };
                let streams_text = !matches!(item, TranscriptItem::Tool { .. });
                self.upsert(item, changed);
                if streams_text {
                    let i = *changed.last().expect("upsert records the index");
                    self.stream_slots.entry(msg).or_default().push_back(i);
                }
            }
            Some("content_block_delta") => {
                let (Some(msg), Some(index), Some(delta)) = (
                    self.streaming_message.as_deref(),
                    event.get("index").and_then(Value::as_u64),
                    event.get("delta"),
                ) else {
                    return;
                };
                let Some(&i) = self.index.get(&format!("{msg}:{index}")) else {
                    return;
                };
                let piece = match str_at(delta, "type") {
                    Some("text_delta") => str_at(delta, "text"),
                    Some("thinking_delta") => str_at(delta, "thinking"),
                    _ => None,
                };
                let (
                    Some(piece),
                    TranscriptItem::Text { text, .. } | TranscriptItem::Thinking { text, .. },
                ) = (piece, &mut self.items[i])
                else {
                    return;
                };
                text.push_str(piece);
                changed.push(i);
            }
            _ => {}
        }
    }

    fn apply_assistant(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        let Some(message) = obj.get("message") else {
            return;
        };
        if let Some(model) = str_at(message, "model").filter(|m| !m.starts_with('<')) {
            self.meta.model = Some(model.to_string());
        }
        if let Some(usage) = message.get("usage") {
            let n = |k| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
            let total =
                n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
            if total > 0 {
                self.meta.context_tokens = Some(total);
            }
        }
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        let msg_id = str_at(message, "id").unwrap_or_default().to_string();
        let uuid = str_at(obj, "uuid").unwrap_or_default();
        for (n, block) in blocks.iter().enumerate() {
            let id = format!("{uuid}:{n}");
            match str_at(block, "type") {
                Some(kind @ ("text" | "thinking")) => {
                    let final_text = str_at(block, kind).unwrap_or_default().to_string();
                    // The streamed item for this block takes the final text in
                    // place, keeping its id so the client updates, not appends.
                    if let Some(i) = self.take_stream_slot(&msg_id) {
                        if let TranscriptItem::Text { text, .. }
                        | TranscriptItem::Thinking { text, .. } = &mut self.items[i]
                        {
                            if !final_text.is_empty() {
                                *text = final_text;
                            }
                        }
                        changed.push(i);
                    } else if !final_text.trim().is_empty() {
                        let item = if kind == "text" {
                            TranscriptItem::Text {
                                id,
                                text: final_text,
                            }
                        } else {
                            TranscriptItem::Thinking {
                                id,
                                text: final_text,
                            }
                        };
                        self.upsert(item, changed);
                    }
                }
                Some("tool_use") => {
                    let tool_id = str_at(block, "id").unwrap_or(&id).to_string();
                    let input = block.get("input").map(clip_value).unwrap_or(Value::Null);
                    match self.index.get(&tool_id) {
                        // Streamed already: fill in the input, keep any result.
                        Some(&i) => {
                            if let TranscriptItem::Tool {
                                input: existing, ..
                            } = &mut self.items[i]
                            {
                                *existing = input;
                            }
                            changed.push(i);
                        }
                        None => self.upsert(
                            TranscriptItem::Tool {
                                id: tool_id,
                                name: str_at(block, "name").unwrap_or("tool").to_string(),
                                input,
                                status: ToolStatus::Running,
                                output: None,
                                full_output_bytes: None,
                                patch: None,
                            },
                            changed,
                        ),
                    }
                }
                _ => continue,
            }
            self.meta.busy = true;
        }
    }

    fn take_stream_slot(&mut self, msg_id: &str) -> Option<usize> {
        self.stream_slots.get_mut(msg_id)?.pop_front()
    }

    fn upsert(&mut self, item: TranscriptItem, changed: &mut Vec<usize>) {
        let i = match self.index.get(item.id()) {
            Some(&i) => {
                // A replayed tool call or approval must not wipe its result.
                if matches!(
                    self.items[i],
                    TranscriptItem::Tool { .. } | TranscriptItem::Approval { .. }
                ) {
                    return;
                }
                self.items[i] = item;
                i
            }
            None => {
                self.index.insert(item.id().to_string(), self.items.len());
                self.items.push(item);
                self.items.len() - 1
            }
        };
        changed.push(i);
    }
}

#[cfg(test)]
mod tests;
