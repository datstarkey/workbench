//! Messages around the conversation rather than in it, folded into
//! [`TranscriptItem::Event`]s (denied tools, hooks that failed or blocked,
//! recalled memories, refusals) and artifact links for the meta.
//!
//! Live sessions report hooks as `system:hook_response`; session JSONL keeps
//! `hook_*` attachments and a `stop_hook_summary` instead. Both paths only
//! surface hooks that blocked, failed or spoke to the user, never a line per
//! successful hook.

use serde_json::Value;

use super::items::{ArtifactInfo, EventKind, TranscriptItem};
use super::parse::{clip, str_at};

/// The item for a `system` message, or `None` when it isn't worth a line.
pub(super) fn system_event(obj: &Value, id: String) -> Option<TranscriptItem> {
    let (kind, title, detail) = match str_at(obj, "subtype")? {
        "permission_denied" => {
            let (kind, title, detail) = permission_denied(obj);
            (kind, title, detail.map(String::from))
        }
        "hook_response" => {
            let (title, detail) = hook_response(obj)?;
            (EventKind::Hook, title, detail)
        }
        "stop_hook_summary" => {
            if obj.get("preventedContinuation").and_then(Value::as_bool) != Some(true) {
                return None;
            }
            let reason = str_at(obj, "stopReason");
            (
                EventKind::Hook,
                "Stop hook stopped Claude".into(),
                reason.map(String::from),
            )
        }
        "memory_recall" => return memory_event(obj.get("memories")?, id),
        "model_refusal_fallback" => {
            let original = str_at(obj, "original_model").unwrap_or("The model");
            let fallback = str_at(obj, "fallback_model").unwrap_or("another model");
            let title = if str_at(obj, "scope") == Some("local") {
                format!("A side request was refused and retried on {fallback}")
            } else {
                format!("{original} refused, retried on {fallback}")
            };
            (
                EventKind::Refusal,
                title,
                refusal_detail(obj).map(String::from),
            )
        }
        "model_refusal_no_fallback" => {
            let original = str_at(obj, "original_model").unwrap_or("The model");
            let title = format!("{original} refused this request");
            (
                EventKind::Refusal,
                title,
                refusal_detail(obj).map(String::from),
            )
        }
        _ => return None,
    };
    Some(event(id, kind, title, detail.as_deref(), Vec::new()))
}

/// The item for a JSONL `attachment`: hook outcomes and recalled memories.
pub(super) fn attachment_event(att: &Value, id: String) -> Option<TranscriptItem> {
    let hook = str_at(att, "hookName")
        .or_else(|| str_at(att, "hookEvent"))
        .unwrap_or("A");
    let (title, detail) = match str_at(att, "type")? {
        "relevant_memories" => return memory_event(att.get("memories")?, id),
        "hook_blocking_error" => (
            format!("{hook} hook blocked"),
            att.pointer("/blockingError/blockingError")
                .and_then(Value::as_str),
        ),
        "hook_non_blocking_error" => {
            let exit = att.get("exitCode").and_then(Value::as_i64);
            (
                failed(hook, exit),
                non_empty(att, "stderr").or(non_empty(att, "stdout")),
            )
        }
        "hook_error_during_execution" => (failed(hook, None), str_at(att, "content")),
        "hook_stopped_continuation" => (
            format!("{hook} hook stopped Claude"),
            str_at(att, "message"),
        ),
        "hook_system_message" => (format!("{hook} hook"), str_at(att, "content")),
        _ => return None,
    };
    Some(event(id, EventKind::Hook, title, detail, Vec::new()))
}

fn permission_denied(obj: &Value) -> (EventKind, String, Option<&str>) {
    let tool = str_at(obj, "tool_name").unwrap_or("a tool");
    let title = match str_at(obj, "decision_reason_type") {
        Some("classifier") => format!("Auto mode blocked {tool}"),
        Some("rule") => format!("A permission rule blocked {tool}"),
        Some("mode") => format!("The permission mode blocked {tool}"),
        Some("asyncAgent") => format!("Blocked {tool}: a background agent can't ask"),
        _ => format!("Blocked {tool}"),
    };
    let detail = non_empty(obj, "decision_reason").or_else(|| str_at(obj, "message"));
    (EventKind::PermissionDenied, title, detail)
}

/// A hook that blocked, failed or showed the user a message. `stdout` may be
/// the hook's JSON reply, which can block without a failing exit code.
fn hook_response(obj: &Value) -> Option<(String, Option<String>)> {
    let hook = str_at(obj, "hook_name")
        .or_else(|| str_at(obj, "hook_event"))
        .unwrap_or("A");
    let exit = obj.get("exit_code").and_then(Value::as_i64);
    let said = non_empty(obj, "stderr")
        .or_else(|| non_empty(obj, "stdout"))
        .or_else(|| non_empty(obj, "output"))
        .map(String::from);
    match str_at(obj, "outcome")? {
        "error" if exit == Some(2) => Some((format!("{hook} hook blocked"), said)),
        "error" => Some((failed(hook, exit), said)),
        "success" => {
            let reply: Value = serde_json::from_str(str_at(obj, "stdout")?.trim()).ok()?;
            if str_at(&reply, "decision") == Some("block")
                || reply.pointer("/hookSpecificOutput/permissionDecision")
                    == Some(&Value::from("deny"))
            {
                let reason = str_at(&reply, "reason").or_else(|| {
                    reply
                        .pointer("/hookSpecificOutput/permissionDecisionReason")
                        .and_then(Value::as_str)
                });
                return Some((format!("{hook} hook blocked"), reason.map(String::from)));
            }
            if reply.get("continue").and_then(Value::as_bool) == Some(false) {
                let reason = str_at(&reply, "stopReason");
                return Some((
                    format!("{hook} hook stopped Claude"),
                    reason.map(String::from),
                ));
            }
            let message = str_at(&reply, "systemMessage")?;
            Some((format!("{hook} hook"), Some(message.to_string())))
        }
        // `cancelled`: the turn was interrupted, which the chat already shows.
        _ => None,
    }
}

fn failed(hook: &str, exit: Option<i64>) -> String {
    match exit {
        Some(code) if code != 0 => format!("{hook} hook failed (exit {code})"),
        _ => format!("{hook} hook failed"),
    }
}

fn refusal_detail(obj: &Value) -> Option<&str> {
    non_empty(obj, "api_refusal_explanation").or_else(|| str_at(obj, "content"))
}

/// Memories surfaced into the turn: file paths (or org URLs) as `files`;
/// synthesised memories have no file, so their text is the detail.
fn memory_event(memories: &Value, id: String) -> Option<TranscriptItem> {
    let list = memories.as_array().filter(|l| !l.is_empty())?;
    let files: Vec<String> = list
        .iter()
        .filter_map(|m| str_at(m, "path"))
        .filter(|p| !p.starts_with("<synthesis"))
        .map(String::from)
        .collect();
    let synthesized: Vec<&str> = list
        .iter()
        .filter(|m| str_at(m, "path").is_none_or(|p| p.starts_with("<synthesis")))
        .filter_map(|m| str_at(m, "content"))
        .collect();
    let title = match files.len() {
        0 => "Recalled from memory".to_string(),
        1 => "Recalled 1 memory".to_string(),
        n => format!("Recalled {n} memories"),
    };
    let detail = (!synthesized.is_empty()).then(|| synthesized.join("\n\n"));
    Some(event(
        id,
        EventKind::Memory,
        title,
        detail.as_deref(),
        files,
    ))
}

/// The artifact an `Artifact` call's structured result points at. Reads,
/// lists and quickstarts carry no top-level `url` and are left out.
pub(super) fn artifact(
    tool_use_id: &str,
    result: &Value,
    output: Option<&str>,
) -> Option<ArtifactInfo> {
    let url =
        str_at(result, "url").filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
    let flag = |k| result.get(k).and_then(Value::as_bool) == Some(true);
    let action = if flag("created_from_type") || output.is_some_and(|o| o.starts_with("Created")) {
        "created"
    } else if flag("updated") {
        "updated"
    } else if flag("opened") {
        "opened"
    } else {
        "published"
    };
    Some(ArtifactInfo {
        tool_use_id: tool_use_id.to_string(),
        url: url.to_string(),
        title: str_at(result, "title")
            .filter(|t| !t.is_empty())
            .map(String::from),
        action: action.into(),
        version: str_at(result, "version").map(String::from),
    })
}

fn non_empty<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    str_at(obj, key).filter(|s| !s.trim().is_empty())
}

fn event(
    id: String,
    event: EventKind,
    title: String,
    detail: Option<&str>,
    files: Vec<String>,
) -> TranscriptItem {
    TranscriptItem::Event {
        id,
        event,
        title,
        detail: detail.map(str::trim).filter(|d| !d.is_empty()).map(clip),
        files,
    }
}

#[cfg(test)]
mod tests;
