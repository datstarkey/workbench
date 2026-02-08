use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

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

        #[cfg(not(unix))]
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

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    use tauri::{AppHandle, Emitter};

    use super::{ClaudeHookEvent, CodexNotifyEvent, HookBridgeEnvelope, HookBridgeState};
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

                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let line = match line {
                        Ok(line) => line,
                        Err(e) => {
                            eprintln!("[HookBridge] Failed to read socket payload: {e}");
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
        });

        HookBridgeState {
            socket_path: Some(socket_path.to_string_lossy().to_string()),
        }
    }
}
