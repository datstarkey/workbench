use std::io::{BufRead, BufReader, Read};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub struct HookBridgeState {
    socket_path: Option<String>,
}

impl HookBridgeState {
    pub fn new(app_handle: AppHandle) -> Self {
        #[cfg(unix)]
        {
            return unix::start(app_handle);
        }

        #[cfg(windows)]
        {
            return tcp::start(app_handle);
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = app_handle;
            Self { socket_path: None }
        }
    }

    pub fn socket_path(&self) -> Option<&str> {
        self.socket_path.as_deref()
    }
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

/// Process lines from a stream, dispatching hook events to the frontend.
/// Shared between Unix socket and TCP implementations.
fn handle_stream<R: Read>(reader: BufReader<R>, handle: &AppHandle) {
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("[HookBridge] Failed to read payload: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let envelope = match serde_json::from_str::<HookBridgeEnvelope>(&line) {
            Ok(envelope) => envelope,
            Err(e) => {
                eprintln!("[HookBridge] Invalid hook payload: {e}");
                continue;
            }
        };

        match envelope {
            HookBridgeEnvelope::Claude { pane_id, hook } => {
                let event = ClaudeHookEvent::from_payload(pane_id, hook);
                let _ = handle.emit("claude:hook", event);
            }
            HookBridgeEnvelope::Codex { pane_id, codex } => {
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

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;

    use tauri::AppHandle;

    use super::{handle_stream, HookBridgeState};
    use crate::paths;

    pub fn start(app_handle: AppHandle) -> HookBridgeState {
        let socket_path = paths::workbench_hook_socket_path();
        if let Err(e) = fs::create_dir_all(paths::workbench_config_dir()) {
            eprintln!("[HookBridge] Failed to create config directory: {e}");
            return HookBridgeState { socket_path: None };
        }

        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("[HookBridge] Failed to bind socket: {e}");
                return HookBridgeState { socket_path: None };
            }
        };

        let handle = app_handle.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(e) => {
                        eprintln!("[HookBridge] Socket accept failed: {e}");
                        continue;
                    }
                };

                let handle = handle.clone();
                std::thread::spawn(move || {
                    handle_stream(BufReader::new(stream), &handle);
                });
            }
        });

        HookBridgeState {
            socket_path: Some(socket_path.to_string_lossy().to_string()),
        }
    }
}

/// TCP-based hook bridge for Windows (and usable as fallback on any platform).
/// Binds to 127.0.0.1:0 (ephemeral port) so there are no port conflicts.
/// Hook scripts connect via AF_INET instead of AF_UNIX.
#[cfg(windows)]
mod tcp {
    use std::io::BufReader;
    use std::net::TcpListener;

    use tauri::AppHandle;

    use super::{handle_stream, HookBridgeState};

    pub fn start(app_handle: AppHandle) -> HookBridgeState {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[HookBridge] Failed to bind TCP listener: {e}");
                return HookBridgeState { socket_path: None };
            }
        };

        let addr = match listener.local_addr() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[HookBridge] Failed to get listener address: {e}");
                return HookBridgeState { socket_path: None };
            }
        };

        let socket_path = format!("127.0.0.1:{}", addr.port());
        let handle = app_handle.clone();

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
                std::thread::spawn(move || {
                    handle_stream(BufReader::new(stream), &handle);
                });
            }
        });

        HookBridgeState {
            socket_path: Some(socket_path),
        }
    }
}
