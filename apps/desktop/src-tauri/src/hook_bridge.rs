//! Shows what the server's hook bridge (`workbench_server::hook_bridge`)
//! hears: a log, a project refresh after a write or git command, and Codex
//! `notify` for the panes' labels. The server owns the listener and stamps it
//! on every process it starts.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use workbench_server::hook_bridge::{HookBridge, HookEvent};

use crate::refresh_dispatcher::RefreshDispatcher;

const MAX_LOG_ENTRIES: usize = 500;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookLogEntry {
    pub timestamp: String,
    pub level: String,
    pub event_name: Option<String>,
    pub pane_id: Option<String>,
    pub source: Option<String>,
    pub summary: String,
    pub tool_name: Option<String>,
}

type LogBuffer = Arc<Mutex<VecDeque<HookLogEntry>>>;

#[derive(Clone)]
pub struct HookBridgeState {
    bridge: HookBridge,
    logs: LogBuffer,
}

impl HookBridgeState {
    /// Follow `bridge` (started here if it isn't yet) for this app's display.
    pub fn new(app_handle: AppHandle, bridge: HookBridge) -> Self {
        bridge.start();
        let state = Self {
            bridge,
            logs: Arc::new(Mutex::new(VecDeque::new())),
        };
        let (mut events, logs) = (state.bridge.subscribe(), state.logs.clone());
        std::thread::spawn(move || loop {
            use tokio::sync::broadcast::error::RecvError;
            match events.blocking_recv() {
                Ok(event) => handle_event(event, &app_handle, &logs),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        });
        state
    }

    pub fn get_logs(&self) -> Vec<HookLogEntry> {
        let logs = self.logs.lock().unwrap_or_else(|e| e.into_inner());
        logs.iter().cloned().collect()
    }

    pub fn clear_logs(&self) {
        let mut logs = self.logs.lock().unwrap_or_else(|e| e.into_inner());
        logs.clear();
    }
}

fn push_log(logs: &LogBuffer, entry: HookLogEntry) {
    let mut buf = logs.lock().unwrap_or_else(|e| e.into_inner());
    if buf.len() >= MAX_LOG_ENTRIES {
        buf.pop_front();
    }
    buf.push_back(entry);
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexNotifyEvent {
    pane_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notify_event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    codex_payload: Value,
}

impl CodexNotifyEvent {
    fn from_payload(pane_id: String, codex_payload: Value) -> Self {
        let session_id = codex_payload
            .get("thread-id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let notify_event = codex_payload
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let cwd = codex_payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            pane_id,
            session_id,
            notify_event,
            cwd,
            codex_payload,
        }
    }
}

fn command_mentions_git_or_gh(command: &str) -> bool {
    command
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|token| token == "git" || token == "gh")
}

fn should_emit_project_refresh_for_hook(hook: &Value) -> bool {
    if hook.get("hook_event_name").and_then(|v| v.as_str()) != Some("PostToolUse") {
        return false;
    }

    match hook.get("tool_name").and_then(|v| v.as_str()) {
        Some("Write" | "Edit" | "NotebookEdit") => true,
        Some("Bash") => {
            let Some(command) = hook
                .get("tool_input")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str())
            else {
                return false;
            };
            command_mentions_git_or_gh(command)
        }
        _ => false,
    }
}

/// Emit a project refresh event. Caller must verify `should_emit_project_refresh_for_hook` first.
fn emit_project_refresh_event(handle: &AppHandle, hook: &Value) {
    // The project to refresh is the hook's cwd at its checkout root (a linked
    // worktree's own root), so a subdirectory cwd keys the path the stores use.
    let Some(project_path) = hook
        .get("cwd")
        .and_then(|v| v.as_str())
        .and_then(|cwd| crate::git::git_info(cwd).ok())
        .map(|info| info.repo_root)
    else {
        return;
    };
    let dispatcher = handle.state::<RefreshDispatcher>();

    let trigger = match hook.get("tool_name").and_then(|v| v.as_str()) {
        Some("Write") => "post-tool-use-write",
        Some("Edit") => "post-tool-use-edit",
        Some("NotebookEdit") => "post-tool-use-notebook-edit",
        _ => "post-tool-use-bash",
    };

    dispatcher.request_refresh(handle, project_path, "claude-hook", trigger);
}

fn emit_log(handle: &AppHandle, logs: &LogBuffer, entry: HookLogEntry) {
    push_log(logs, entry.clone());
    let _ = handle.emit("hook-bridge:log", entry);
}

/// Show one event: log it, refresh its project, pass Codex `notify` on.
fn handle_event(event: HookEvent, handle: &AppHandle, logs: &LogBuffer) {
    match event {
        HookEvent::Invalid { summary } => emit_log(
            handle,
            logs,
            HookLogEntry {
                timestamp: Utc::now().to_rfc3339(),
                level: "error".into(),
                event_name: None,
                pane_id: None,
                source: None,
                summary,
                tool_name: None,
            },
        ),
        HookEvent::Claude { pane_id, hook } => {
            let refreshed = should_emit_project_refresh_for_hook(&hook);
            if refreshed {
                emit_project_refresh_event(handle, &hook);
            }

            let event_name = hook
                .get("hook_event_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let tool_name = hook
                .get("tool_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let summary = match (&event_name, &tool_name) {
                (Some(ev), Some(tool)) => {
                    let mut s = format!("{ev}: {tool}");
                    if ev == "PostToolUse" && tool == "Bash" {
                        if let Some(cmd) = hook
                            .get("tool_input")
                            .and_then(|v| v.get("command"))
                            .and_then(|v| v.as_str())
                        {
                            let display = if cmd.len() > 80 {
                                format!("{}…", crate::text::truncate_bytes(cmd, 80))
                            } else {
                                cmd.to_string()
                            };
                            s = format!("{s} — {display}");
                        }
                    }
                    if refreshed {
                        s.push_str(" → refreshed");
                    }
                    s
                }
                (Some(ev), None) => ev.clone(),
                _ => "Claude hook event".into(),
            };
            let log_entry = HookLogEntry {
                timestamp: Utc::now().to_rfc3339(),
                level: "event".into(),
                event_name,
                pane_id: Some(pane_id.clone()),
                source: Some("claude".into()),
                summary,
                tool_name,
            };
            emit_log(handle, logs, log_entry);
        }
        HookEvent::Codex { pane_id, codex } => {
            let event_name = codex
                .get("type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let summary = event_name
                .clone()
                .unwrap_or_else(|| "Codex notification".into());
            let log_entry = HookLogEntry {
                timestamp: Utc::now().to_rfc3339(),
                level: "event".into(),
                event_name,
                pane_id: Some(pane_id.clone()),
                source: Some("codex".into()),
                summary,
                tool_name: None,
            };
            emit_log(handle, logs, log_entry);

            // Labels only: the server puts a finished turn in the attention feed.
            let event = CodexNotifyEvent::from_payload(pane_id, codex);
            let _ = handle.emit("codex:notify", event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_log_entry(summary: &str) -> HookLogEntry {
        HookLogEntry {
            timestamp: "2025-01-01T00:00:00Z".into(),
            level: "event".into(),
            event_name: None,
            pane_id: None,
            source: None,
            summary: summary.into(),
            tool_name: None,
        }
    }

    // --- Log buffer ---

    #[test]
    fn log_buffer_push_and_get() {
        let logs: LogBuffer = Arc::new(Mutex::new(VecDeque::new()));
        push_log(&logs, make_log_entry("SessionStart"));

        let buf = logs.lock().unwrap();
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0].summary, "SessionStart");
    }

    #[test]
    fn log_buffer_evicts_oldest_at_capacity() {
        let logs: LogBuffer = Arc::new(Mutex::new(VecDeque::new()));
        for i in 0..MAX_LOG_ENTRIES + 50 {
            push_log(&logs, make_log_entry(&format!("entry-{i}")));
        }

        let buf = logs.lock().unwrap();
        assert_eq!(buf.len(), MAX_LOG_ENTRIES);
        assert_eq!(buf[0].summary, "entry-50");
        assert_eq!(buf[MAX_LOG_ENTRIES - 1].summary, "entry-549");
    }

    #[test]
    fn log_state_get_returns_clone() {
        let state = HookBridgeState {
            bridge: Default::default(),
            logs: Arc::new(Mutex::new(VecDeque::new())),
        };
        push_log(&state.logs, make_log_entry("test"));
        let result = state.get_logs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].summary, "test");
    }

    #[test]
    fn log_state_clear() {
        let state = HookBridgeState {
            bridge: Default::default(),
            logs: Arc::new(Mutex::new(VecDeque::new())),
        };
        push_log(&state.logs, make_log_entry("one"));
        push_log(&state.logs, make_log_entry("two"));
        assert_eq!(state.get_logs().len(), 2);

        state.clear_logs();
        assert_eq!(state.get_logs().len(), 0);
    }

    // --- CodexNotifyEvent::from_payload ---

    #[test]
    fn codex_notify_with_hyphenated_thread_id() {
        let payload = json!({
            "thread-id": "thread-abc",
            "type": "agent-turn-complete",
            "turn-id": "turn-1",
            "cwd": "/home/user/proj"
        });
        let event = CodexNotifyEvent::from_payload("pane-5".into(), payload.clone());

        assert_eq!(event.pane_id, "pane-5");
        assert_eq!(event.session_id.as_deref(), Some("thread-abc"));
        assert_eq!(event.notify_event.as_deref(), Some("agent-turn-complete"));
        assert_eq!(event.cwd.as_deref(), Some("/home/user/proj"));
        assert_eq!(event.codex_payload, payload);
    }

    #[test]
    fn codex_notify_missing_thread_id() {
        let payload = json!({"type": "agent-turn-complete"});
        let event = CodexNotifyEvent::from_payload("pane-8".into(), payload);

        assert!(event.session_id.is_none());
        assert_eq!(event.notify_event.as_deref(), Some("agent-turn-complete"));
    }

    #[test]
    fn codex_notify_empty_payload() {
        let payload = json!({});
        let event = CodexNotifyEvent::from_payload("pane-9".into(), payload);

        assert!(event.session_id.is_none());
        assert!(event.notify_event.is_none());
        assert!(event.cwd.is_none());
    }

    // --- refresh trigger detection ---

    #[test]
    fn refresh_trigger_post_tool_use_bash_git() {
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "git status" }
        });
        assert!(should_emit_project_refresh_for_hook(&payload));
    }

    #[test]
    fn refresh_trigger_post_tool_use_bash_gh() {
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "gh pr status" }
        });
        assert!(should_emit_project_refresh_for_hook(&payload));
    }

    #[test]
    fn refresh_trigger_post_tool_use_write() {
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Write",
            "tool_input": { "file_path": "/repo/src/main.rs", "content": "fn main() {}" }
        });
        assert!(should_emit_project_refresh_for_hook(&payload));
    }

    #[test]
    fn refresh_trigger_post_tool_use_edit() {
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Edit",
            "tool_input": { "file_path": "/repo/src/main.rs", "old_string": "a", "new_string": "b" }
        });
        assert!(should_emit_project_refresh_for_hook(&payload));
    }

    #[test]
    fn refresh_trigger_post_tool_use_notebook_edit() {
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "NotebookEdit",
            "tool_input": { "notebook_path": "/repo/notebook.ipynb" }
        });
        assert!(should_emit_project_refresh_for_hook(&payload));
    }

    #[test]
    fn refresh_trigger_rejects_non_post_tool_use() {
        let payload = json!({
            "hook_event_name": "Notification",
            "tool_name": "Bash",
            "tool_input": { "command": "git status" }
        });
        assert!(!should_emit_project_refresh_for_hook(&payload));
    }

    #[test]
    fn refresh_trigger_rejects_read_only_tools() {
        for tool in &["Read", "Grep", "Glob", "WebSearch"] {
            let payload = json!({
                "hook_event_name": "PostToolUse",
                "tool_name": tool,
                "tool_input": {}
            });
            assert!(
                !should_emit_project_refresh_for_hook(&payload),
                "should reject {tool}"
            );
        }
    }

    #[test]
    fn refresh_trigger_rejects_non_git_commands() {
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "echo hello" }
        });
        assert!(!should_emit_project_refresh_for_hook(&payload));
    }
}
