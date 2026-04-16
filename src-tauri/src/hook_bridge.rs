use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::pty::PtyManager;
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
    socket_path: Option<String>,
    logs: LogBuffer,
}

impl HookBridgeState {
    pub fn new(app_handle: AppHandle) -> Self {
        tcp::start(app_handle)
    }

    pub fn socket_path(&self) -> Option<&str> {
        self.socket_path.as_deref()
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HookBridgeEnvelope {
    Claude { pane_id: String, hook: Value },
    Codex { pane_id: String, codex: Value },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeHookEvent {
    pane_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_event_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript_path: Option<String>,
    hook_payload: Value,
}

impl ClaudeHookEvent {
    fn from_payload(pane_id: String, hook_payload: Value) -> Self {
        let session_id = hook_payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let hook_event_name = hook_payload
            .get("hook_event_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let source = hook_payload
            .get("source")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let cwd = hook_payload
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let transcript_path = hook_payload
            .get("transcript_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            pane_id,
            session_id,
            hook_event_name,
            source,
            cwd,
            transcript_path,
            hook_payload,
        }
    }
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
            .or_else(|| codex_payload.get("thread_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let notify_event = codex_payload
            .get("event")
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
    if hook
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        != Some("PostToolUse")
    {
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
fn emit_project_refresh_event(handle: &AppHandle, pane_id: &str, hook: &Value) {
    let pty_manager = handle.state::<PtyManager>();
    let Some(project_path) = pty_manager.project_path_for_session(pane_id) else {
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

/// Process lines from a stream, dispatching hook events to the frontend.
/// Shared between Unix socket and TCP implementations.
fn handle_stream<R: Read>(reader: BufReader<R>, handle: &AppHandle, logs: &LogBuffer) {
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                let entry = HookLogEntry {
                    timestamp: Utc::now().to_rfc3339(),
                    level: "error".into(),
                    event_name: None,
                    pane_id: None,
                    source: None,
                    summary: format!("Failed to read payload: {e}"),
                    tool_name: None,
                };
                push_log(logs, entry.clone());
                let _ = handle.emit("hook-bridge:log", entry);
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let envelope = match serde_json::from_str::<HookBridgeEnvelope>(&line) {
            Ok(envelope) => envelope,
            Err(e) => {
                let truncated = if line.len() > 200 {
                    format!("{}…", &line[..200])
                } else {
                    line.clone()
                };
                let entry = HookLogEntry {
                    timestamp: Utc::now().to_rfc3339(),
                    level: "error".into(),
                    event_name: None,
                    pane_id: None,
                    source: None,
                    summary: format!("Invalid payload: {e} — {truncated}"),
                    tool_name: None,
                };
                push_log(logs, entry.clone());
                let _ = handle.emit("hook-bridge:log", entry);
                continue;
            }
        };

        match envelope {
            HookBridgeEnvelope::Claude { pane_id, hook } => {
                let refreshed = should_emit_project_refresh_for_hook(&hook);
                if refreshed {
                    emit_project_refresh_event(handle, &pane_id, &hook);
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
                                    format!("{}…", &cmd[..80])
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
                push_log(logs, log_entry.clone());
                let _ = handle.emit("hook-bridge:log", log_entry);

                let event = ClaudeHookEvent::from_payload(pane_id, hook);
                let _ = handle.emit("claude:hook", event);
            }
            HookBridgeEnvelope::Codex { pane_id, codex } => {
                let event_name = codex
                    .get("event")
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
                push_log(logs, log_entry.clone());
                let _ = handle.emit("hook-bridge:log", log_entry);

                let event = CodexNotifyEvent::from_payload(pane_id, codex);
                let _ = handle.emit("codex:notify", event);
            }
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
            socket_path: None,
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
            socket_path: None,
            logs: Arc::new(Mutex::new(VecDeque::new())),
        };
        push_log(&state.logs, make_log_entry("one"));
        push_log(&state.logs, make_log_entry("two"));
        assert_eq!(state.get_logs().len(), 2);

        state.clear_logs();
        assert_eq!(state.get_logs().len(), 0);
    }

    // --- ClaudeHookEvent::from_payload ---

    #[test]
    fn claude_hook_all_fields() {
        let payload = json!({
            "session_id": "abc-123",
            "hook_event_name": "tool_use",
            "source": "claude",
            "cwd": "/Users/jake/project",
            "transcript_path": "/tmp/transcript.jsonl",
            "extra_field": true
        });
        let event = ClaudeHookEvent::from_payload("pane-1".into(), payload.clone());

        assert_eq!(event.pane_id, "pane-1");
        assert_eq!(event.session_id.as_deref(), Some("abc-123"));
        assert_eq!(event.hook_event_name.as_deref(), Some("tool_use"));
        assert_eq!(event.source.as_deref(), Some("claude"));
        assert_eq!(event.cwd.as_deref(), Some("/Users/jake/project"));
        assert_eq!(
            event.transcript_path.as_deref(),
            Some("/tmp/transcript.jsonl")
        );
        assert_eq!(event.hook_payload, payload);
    }

    #[test]
    fn claude_hook_missing_optional_fields() {
        let payload = json!({"session_id": "s1"});
        let event = ClaudeHookEvent::from_payload("pane-2".into(), payload);

        assert_eq!(event.session_id.as_deref(), Some("s1"));
        assert!(event.hook_event_name.is_none());
        assert!(event.source.is_none());
        assert!(event.cwd.is_none());
        assert!(event.transcript_path.is_none());
    }

    #[test]
    fn claude_hook_empty_payload() {
        let payload = json!({});
        let event = ClaudeHookEvent::from_payload("pane-3".into(), payload);

        assert_eq!(event.pane_id, "pane-3");
        assert!(event.session_id.is_none());
        assert!(event.hook_event_name.is_none());
        assert!(event.source.is_none());
        assert!(event.cwd.is_none());
        assert!(event.transcript_path.is_none());
    }

    #[test]
    fn claude_hook_non_string_values_ignored() {
        let payload = json!({"session_id": 42, "cwd": true});
        let event = ClaudeHookEvent::from_payload("pane-4".into(), payload);

        assert!(event.session_id.is_none());
        assert!(event.cwd.is_none());
    }

    // --- CodexNotifyEvent::from_payload ---

    #[test]
    fn codex_notify_with_hyphenated_thread_id() {
        let payload = json!({
            "thread-id": "thread-abc",
            "event": "task_complete",
            "cwd": "/home/user/proj"
        });
        let event = CodexNotifyEvent::from_payload("pane-5".into(), payload.clone());

        assert_eq!(event.pane_id, "pane-5");
        assert_eq!(event.session_id.as_deref(), Some("thread-abc"));
        assert_eq!(event.notify_event.as_deref(), Some("task_complete"));
        assert_eq!(event.cwd.as_deref(), Some("/home/user/proj"));
        assert_eq!(event.codex_payload, payload);
    }

    #[test]
    fn codex_notify_with_underscore_thread_id() {
        let payload = json!({"thread_id": "thread-xyz"});
        let event = CodexNotifyEvent::from_payload("pane-6".into(), payload);

        // thread_id is used as fallback when thread-id is absent
        assert_eq!(event.session_id.as_deref(), Some("thread-xyz"));
    }

    #[test]
    fn codex_notify_both_thread_id_forms_hyphen_wins() {
        let payload = json!({
            "thread-id": "hyphen-wins",
            "thread_id": "underscore-loses"
        });
        let event = CodexNotifyEvent::from_payload("pane-7".into(), payload);

        // Code checks thread-id first via or_else, so hyphen takes priority
        assert_eq!(event.session_id.as_deref(), Some("hyphen-wins"));
    }

    #[test]
    fn codex_notify_missing_thread_id() {
        let payload = json!({"event": "idle"});
        let event = CodexNotifyEvent::from_payload("pane-8".into(), payload);

        assert!(event.session_id.is_none());
        assert_eq!(event.notify_event.as_deref(), Some("idle"));
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
            assert!(!should_emit_project_refresh_for_hook(&payload), "should reject {tool}");
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

    // --- HookBridgeEnvelope deserialization ---

    #[test]
    fn envelope_claude_hook() {
        let json_str = r#"{"pane_id": "p1", "hook": {"session_id": "s1"}}"#;
        let envelope: HookBridgeEnvelope = serde_json::from_str(json_str).unwrap();

        match envelope {
            HookBridgeEnvelope::Claude { pane_id, hook } => {
                assert_eq!(pane_id, "p1");
                assert_eq!(hook.get("session_id").unwrap().as_str().unwrap(), "s1");
            }
            HookBridgeEnvelope::Codex { .. } => panic!("Expected Claude variant"),
        }
    }

    #[test]
    fn envelope_codex_notify() {
        let json_str = r#"{"pane_id": "p2", "codex": {"thread-id": "t1"}}"#;
        let envelope: HookBridgeEnvelope = serde_json::from_str(json_str).unwrap();

        match envelope {
            HookBridgeEnvelope::Codex { pane_id, codex } => {
                assert_eq!(pane_id, "p2");
                assert_eq!(codex.get("thread-id").unwrap().as_str().unwrap(), "t1");
            }
            HookBridgeEnvelope::Claude { .. } => panic!("Expected Codex variant"),
        }
    }

    #[test]
    fn envelope_missing_pane_id_fails() {
        let json_str = r#"{"hook": {"session_id": "s1"}}"#;
        assert!(serde_json::from_str::<HookBridgeEnvelope>(json_str).is_err());
    }

    #[test]
    fn envelope_no_hook_or_codex_fails() {
        let json_str = r#"{"pane_id": "p3"}"#;
        assert!(serde_json::from_str::<HookBridgeEnvelope>(json_str).is_err());
    }
}

/// TCP-based hook bridge for all platforms.
/// Binds to 127.0.0.1:0 (ephemeral port) so there are no port conflicts.
/// Hook scripts connect via TCP using /dev/tcp (bash) or TcpClient (PowerShell).
mod tcp {
    use std::collections::VecDeque;
    use std::io::BufReader;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use tauri::AppHandle;

    use super::{handle_stream, HookBridgeState};

    pub fn start(app_handle: AppHandle) -> HookBridgeState {
        let logs = Arc::new(Mutex::new(VecDeque::new()));

        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[HookBridge] Failed to bind TCP listener: {e}");
                return HookBridgeState {
                    socket_path: None,
                    logs,
                };
            }
        };

        let addr = match listener.local_addr() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[HookBridge] Failed to get listener address: {e}");
                return HookBridgeState {
                    socket_path: None,
                    logs,
                };
            }
        };

        let socket_path = format!("127.0.0.1:{}", addr.port());
        let handle = app_handle.clone();
        let logs_clone = logs.clone();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[HookBridge] TCP accept failed: {e}");
                        continue;
                    }
                };

                let handle = handle.clone();
                let logs = logs_clone.clone();
                std::thread::spawn(move || {
                    handle_stream(BufReader::new(stream), &handle, &logs);
                });
            }
        });

        HookBridgeState {
            socket_path: Some(socket_path),
            logs,
        }
    }
}
