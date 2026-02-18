use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::Command;
use std::sync::mpsc::{RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};

use crate::types::{TerminalActivityEvent, TerminalDataEvent, TerminalExitEvent};

const PTY_READ_BUFFER_SIZE: usize = 32768;
const STARTUP_COMMAND_DELAY_MS: u64 = 300;
const TERMINAL_QUIET_THRESHOLD_MS: u64 = 1000;
const PTY_DATA_CHANNEL_CAPACITY: usize = 256;

#[derive(Clone, Copy)]
enum ActivitySignal {
    Data,
    Timeout,
    Disconnected,
}

fn default_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    }
    #[cfg(windows)]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    }
}

fn update_activity_state(
    session_id: &str,
    active: bool,
    signal: ActivitySignal,
) -> (bool, Option<TerminalActivityEvent>) {
    match (active, signal) {
        (false, ActivitySignal::Data) => (
            true,
            Some(TerminalActivityEvent {
                session_id: session_id.to_string(),
                active: true,
            }),
        ),
        (true, ActivitySignal::Timeout) | (true, ActivitySignal::Disconnected) => (
            false,
            Some(TerminalActivityEvent {
                session_id: session_id.to_string(),
                active: false,
            }),
        ),
        _ => (active, None),
    }
}

fn resolve_repo_root(path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .env("PATH", crate::paths::enriched_path())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let repo_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if repo_root.is_empty() {
        None
    } else {
        Some(repo_root)
    }
}

fn send_output_chunk(tx: &SyncSender<String>, data: String) -> bool {
    match tx.try_send(data) {
        Ok(()) => true,
        Err(TrySendError::Full(data)) => tx.send(data).is_ok(),
        Err(TrySendError::Disconnected(_)) => false,
    }
}

struct PtySession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

type SessionMap = Arc<Mutex<HashMap<String, Arc<Mutex<PtySession>>>>>;
type SessionProjectMap = Arc<Mutex<HashMap<String, String>>>;

/// Manages PTY sessions with per-session locking so operations on one terminal
/// never block another. The outer map lock is only held briefly for
/// insert/remove/lookup — never during I/O.
pub struct PtyManager {
    sessions: SessionMap,
    session_project_paths: SessionProjectMap,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_project_paths: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a reference to a session by ID. Locks the map only briefly.
    fn get_session(&self, session_id: &str) -> Option<Arc<Mutex<PtySession>>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
    }

    /// Remove a session from the map. Returns the session if it existed.
    fn remove_session(sessions: &SessionMap, session_id: &str) -> Option<Arc<Mutex<PtySession>>> {
        sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id)
    }

    pub fn project_path_for_session(&self, session_id: &str) -> Option<String> {
        self.session_project_paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
    }

    pub fn spawn(
        &self,
        session_id: String,
        project_path: String,
        shell: String,
        cols: u16,
        rows: u16,
        startup_command: Option<String>,
        hook_socket_path: Option<String>,
        app_handle: AppHandle,
    ) -> Result<()> {
        let pty_system = native_pty_system();
        let resolved_project_path = resolve_repo_root(&project_path).unwrap_or(project_path.clone());

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(size).context("Failed to open PTY")?;

        let shell_path = if shell.is_empty() {
            default_shell()
        } else {
            shell
        };

        let mut cmd = CommandBuilder::new(&shell_path);
        #[cfg(unix)]
        cmd.arg("-l");
        cmd.cwd(&project_path);

        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
        #[cfg(unix)]
        {
            if let Ok(home) = std::env::var("HOME") {
                cmd.env("HOME", home);
            }
            if let Ok(user) = std::env::var("USER") {
                cmd.env("USER", user);
            }
            cmd.env("TERM", "xterm-256color");
            cmd.env("COLORTERM", "truecolor");
            cmd.env(
                "LANG",
                std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string()),
            );
        }
        #[cfg(windows)]
        {
            if let Ok(userprofile) = std::env::var("USERPROFILE") {
                cmd.env("USERPROFILE", userprofile);
            }
            if let Ok(username) = std::env::var("USERNAME") {
                cmd.env("USERNAME", username);
            }
            if let Ok(appdata) = std::env::var("APPDATA") {
                cmd.env("APPDATA", appdata);
            }
            if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
                cmd.env("LOCALAPPDATA", localappdata);
            }
            if let Ok(systemroot) = std::env::var("SystemRoot") {
                cmd.env("SystemRoot", systemroot);
            }
        }
        cmd.env("WORKBENCH_PANE_ID", session_id.clone());
        if let Some(socket_path) = hook_socket_path {
            cmd.env("WORKBENCH_HOOK_SOCKET", socket_path);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn shell")?;

        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .context("Failed to get PTY writer")?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("Failed to get PTY reader")?;

        let session = Arc::new(Mutex::new(PtySession {
            writer,
            master: pair.master,
            child,
        }));

        // Insert into map before spawning threads
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.clone(), Arc::clone(&session));
        self.session_project_paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.clone(), resolved_project_path);

        // Two-thread output pipeline (same approach as Alacritty / Kitty):
        //   Reader  — drains the PTY as fast as possible (no sleeps, no backpressure)
        //   Emitter — coalesces output and emits to the frontend at a controlled rate
        //
        // During heavy streaming the emitter batches data that accumulated in the
        // channel while it was busy with the previous emit, naturally reducing the
        // number of IPC events and freeing the WebView bridge for input writes.
        // During light activity the emitter fires immediately (no added latency).

        let (data_tx, data_rx) = std::sync::mpsc::sync_channel::<String>(PTY_DATA_CHANNEL_CAPACITY);
        let (activity_tx, activity_rx) = std::sync::mpsc::channel::<()>();

        let activity_sid = session_id.clone();
        let activity_handle = app_handle.clone();
        std::thread::spawn(move || {
            let quiet_window = Duration::from_millis(TERMINAL_QUIET_THRESHOLD_MS);
            let mut active = false;

            loop {
                let signal = match activity_rx.recv_timeout(quiet_window) {
                    Ok(()) => ActivitySignal::Data,
                    Err(RecvTimeoutError::Timeout) => ActivitySignal::Timeout,
                    Err(RecvTimeoutError::Disconnected) => ActivitySignal::Disconnected,
                };

                let (next_active, event) = update_activity_state(&activity_sid, active, signal);
                if let Some(payload) = event {
                    let _ = activity_handle.emit("terminal:activity", payload);
                }
                active = next_active;

                if matches!(signal, ActivitySignal::Disconnected) {
                    break;
                }
            }
        });

        // ── Reader thread ────────────────────────────────────────────────
        std::thread::spawn(move || {
            let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
            let mut carry = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = if carry.is_empty() {
                            &buf[..n]
                        } else {
                            carry.extend_from_slice(&buf[..n]);
                            carry.as_slice()
                        };

                        let valid_up_to = match std::str::from_utf8(chunk) {
                            Ok(_) => chunk.len(),
                            Err(e) => e.valid_up_to(),
                        };

                        if valid_up_to > 0 {
                            let data = unsafe {
                                std::str::from_utf8_unchecked(&chunk[..valid_up_to])
                            };
                            if !send_output_chunk(&data_tx, data.to_string()) {
                                break; // emitter gone
                            }
                        }

                        let remainder = &chunk[valid_up_to..];
                        carry = remainder.to_vec();
                    }
                    Err(_) => break,
                }
            }
        });

        // ── Emitter thread ───────────────────────────────────────────────
        let sid = session_id.clone();
        let handle = app_handle.clone();
        let sessions_for_cleanup = Arc::clone(&self.sessions);
        let session_project_paths_for_cleanup = Arc::clone(&self.session_project_paths);
        let session_for_cleanup = Arc::clone(&session);

        std::thread::spawn(move || {
            /// During fast output, yield briefly so the reader can fill the
            /// channel with more data, reducing the total number of IPC events.
            const FAST_THRESHOLD: Duration = Duration::from_millis(8);
            const COALESCE_YIELD: Duration = Duration::from_millis(2);

            let mut batch = String::new();
            let mut last_emit = Instant::now();

            loop {
                // Block until the reader pushes data (or closes the channel).
                match data_rx.recv() {
                    Ok(data) => batch.push_str(&data),
                    Err(_) => break, // reader EOF / closed
                }

                // Drain everything the reader has queued so far.
                while let Ok(data) = data_rx.try_recv() {
                    batch.push_str(&data);
                }

                // If output is flowing fast, yield to let more accumulate.
                if last_emit.elapsed() < FAST_THRESHOLD {
                    std::thread::sleep(COALESCE_YIELD);
                    while let Ok(data) = data_rx.try_recv() {
                        batch.push_str(&data);
                    }
                }

                if !batch.is_empty() {
                    let _ = activity_tx.send(());
                    let _ = handle.emit(
                        "terminal:data",
                        TerminalDataEvent {
                            session_id: sid.clone(),
                            data: std::mem::take(&mut batch),
                        },
                    );
                    last_emit = Instant::now();
                }
            }

            // Flush any remaining data in the channel.
            while let Ok(data) = data_rx.try_recv() {
                batch.push_str(&data);
            }
            if !batch.is_empty() {
                let _ = activity_tx.send(());
                let _ = handle.emit(
                    "terminal:data",
                    TerminalDataEvent {
                        session_id: sid.clone(),
                        data: batch,
                    },
                );
            }

            // Cleanup: remove session from map and emit exit event.
            Self::remove_session(&sessions_for_cleanup, &sid);
            session_project_paths_for_cleanup
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&sid);

            let exit_code = session_for_cleanup
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .child
                .wait()
                .map(|s| if s.success() { 0 } else { 1 })
                .unwrap_or(1);

            let _ = handle.emit(
                "terminal:exit",
                TerminalExitEvent {
                    session_id: sid,
                    exit_code,
                    signal: None,
                },
            );
        });

        // Write startup command after a small delay
        if let Some(cmd_str) = startup_command {
            let session_ref = Arc::clone(&session);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(STARTUP_COMMAND_DELAY_MS));
                if let Ok(mut sess) = session_ref.lock() {
                    let cmd_with_newline = format!("{}\n", cmd_str);
                    let _ = sess.writer.write_all(cmd_with_newline.as_bytes());
                }
            });
        }

        Ok(())
    }

    pub fn write(&self, session_id: &str, data: &str) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;
        let mut sess = session.lock().unwrap_or_else(|e| e.into_inner());
        sess.writer.write_all(data.as_bytes())?;
        sess.writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;
        let sess = session.lock().unwrap_or_else(|e| e.into_inner());
        sess.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn kill(&self, session_id: &str, app_handle: &AppHandle) -> Result<()> {
        let session = match Self::remove_session(&self.sessions, session_id) {
            Some(s) => s,
            None => return Ok(()), // already cleaned up by reader thread
        };
        self.session_project_paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);

        let mut sess = session.lock().unwrap_or_else(|e| e.into_inner());
        let _ = sess.child.kill();
        let exit_code = sess
            .child
            .wait()
            .map(|s| if s.success() { 0 } else { 1 })
            .unwrap_or(1);

        let _ = app_handle.emit(
            "terminal:exit",
            TerminalExitEvent {
                session_id: session_id.to_string(),
                exit_code,
                signal: None,
            },
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_returns_nonempty() {
        let shell = default_shell();
        assert!(!shell.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn default_shell_unix_returns_shell_path() {
        let shell = default_shell();
        // Should be a path like /bin/zsh or /bin/bash
        assert!(
            shell.starts_with('/') || shell.contains("sh"),
            "Unexpected Unix shell: {shell}"
        );
    }

    #[test]
    fn update_activity_state_emits_active_on_first_data() {
        let (active, event) = update_activity_state("pane-1", false, ActivitySignal::Data);
        assert!(active);
        assert!(event.is_some());
        let payload = event.unwrap();
        assert_eq!(payload.session_id, "pane-1");
        assert!(payload.active);
    }

    #[test]
    fn update_activity_state_emits_inactive_on_timeout_when_active() {
        let (active, event) = update_activity_state("pane-1", true, ActivitySignal::Timeout);
        assert!(!active);
        assert!(event.is_some());
        let payload = event.unwrap();
        assert_eq!(payload.session_id, "pane-1");
        assert!(!payload.active);
    }

    #[test]
    fn update_activity_state_noop_when_already_inactive_and_timeout() {
        let (active, event) = update_activity_state("pane-1", false, ActivitySignal::Timeout);
        assert!(!active);
        assert!(event.is_none());
    }

    #[test]
    fn update_activity_state_noop_when_already_active_and_data() {
        let (active, event) = update_activity_state("pane-1", true, ActivitySignal::Data);
        assert!(active);
        assert!(event.is_none());
    }

    #[test]
    fn update_activity_state_emits_inactive_on_disconnect_when_active() {
        let (active, event) = update_activity_state("pane-1", true, ActivitySignal::Disconnected);
        assert!(!active);
        assert!(event.is_some());
        assert!(!event.unwrap().active);
    }

    #[test]
    fn send_output_chunk_returns_false_when_receiver_dropped() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1);
        drop(rx);
        assert!(!send_output_chunk(&tx, "data".to_string()));
    }

    #[test]
    fn send_output_chunk_blocks_to_preserve_data_when_channel_full() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1);
        tx.send("first".to_string()).unwrap();

        let sender = tx.clone();
        let handle = std::thread::spawn(move || send_output_chunk(&sender, "second".to_string()));

        let first = rx.recv().unwrap();
        assert_eq!(first, "first");
        let result = handle.join().unwrap();
        assert!(result);
        let second = rx.recv().unwrap();
        assert_eq!(second, "second");
    }

    #[cfg(windows)]
    #[test]
    fn default_shell_windows_returns_known_shell() {
        let shell = default_shell();
        let lower = shell.to_lowercase();
        assert!(
            lower.contains("cmd") || lower.contains("powershell") || lower.contains("pwsh"),
            "Unexpected Windows shell: {shell}"
        );
    }
}
