use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};

use crate::types::{TerminalDataEvent, TerminalExitEvent};

const PTY_READ_BUFFER_SIZE: usize = 8192;
const STARTUP_COMMAND_DELAY_MS: u64 = 300;

struct PtySession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

type SessionMap = Arc<Mutex<HashMap<String, Arc<Mutex<PtySession>>>>>;

/// Manages PTY sessions with per-session locking so operations on one terminal
/// never block another. The outer map lock is only held briefly for
/// insert/remove/lookup — never during I/O.
pub struct PtyManager {
    sessions: SessionMap,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
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

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(size).context("Failed to open PTY")?;

        let shell_path = if shell.is_empty() {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
        } else {
            shell
        };

        let mut cmd = CommandBuilder::new(&shell_path);
        cmd.arg("-l");
        cmd.cwd(&project_path);

        if let Ok(home) = std::env::var("HOME") {
            cmd.env("HOME", home);
        }
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
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

        // Reader thread — emits terminal:data events, then signals cleanup on EOF
        let sid = session_id.clone();
        let handle = app_handle.clone();
        let sessions_for_cleanup = Arc::clone(&self.sessions);
        let session_for_cleanup = Arc::clone(&session);

        std::thread::spawn(move || {
            // Read loop
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
                            // Safety: validated this range above
                            let data =
                                unsafe { std::str::from_utf8_unchecked(&chunk[..valid_up_to]) };
                            let _ = handle.emit(
                                "terminal:data",
                                TerminalDataEvent {
                                    session_id: sid.clone(),
                                    data: data.to_string(),
                                },
                            );
                        }

                        let remainder = &chunk[valid_up_to..];
                        carry = remainder.to_vec();
                    }
                    Err(_) => break,
                }
            }

            // Reader done — remove session from map and emit exit event
            Self::remove_session(&sessions_for_cleanup, &sid);

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
