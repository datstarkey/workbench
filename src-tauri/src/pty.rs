use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};

use crate::types::{TerminalDataEvent, TerminalExitEvent};

struct PtySession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn spawn(
        &self,
        session_id: String,
        project_path: String,
        shell: String,
        cols: u16,
        rows: u16,
        startup_command: Option<String>,
        app_handle: AppHandle,
    ) -> Result<()> {
        let pty_system = native_pty_system();

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system
            .openpty(size)
            .context("Failed to open PTY")?;

        let shell_path = if shell.is_empty() {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
        } else {
            shell
        };

        let mut cmd = CommandBuilder::new(&shell_path);
        cmd.arg("-l");
        cmd.cwd(&project_path);

        // Inherit important environment variables
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
        cmd.env("LANG", std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string()));

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn shell")?;

        // Drop the slave — we only need the master side
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .context("Failed to get PTY writer")?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("Failed to get PTY reader")?;

        // Reader thread — reads from PTY and emits events to frontend.
        // Handles UTF-8 boundaries: if a multi-byte character is split across
        // reads, the trailing incomplete bytes are carried over to the next read.
        let sid = session_id.clone();
        let handle = app_handle.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut carry = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        // Prepend any leftover bytes from the previous read
                        let chunk = if carry.is_empty() {
                            &buf[..n]
                        } else {
                            carry.extend_from_slice(&buf[..n]);
                            carry.as_slice()
                        };

                        // Find the last valid UTF-8 boundary
                        let valid_up_to = match std::str::from_utf8(chunk) {
                            Ok(_) => chunk.len(),
                            Err(e) => e.valid_up_to(),
                        };

                        if valid_up_to > 0 {
                            // Safety: we just validated this range is valid UTF-8
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

                        // Save any trailing incomplete bytes for the next read
                        let remainder = &chunk[valid_up_to..];
                        carry = remainder.to_vec();
                    }
                    Err(_) => break,
                }
            }
        });

        // If there's a startup command, write it after a small delay
        let sessions = self.sessions.clone();
        if let Some(cmd_str) = startup_command {
            let sid = session_id.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if let Ok(mut sessions) = sessions.lock() {
                    if let Some(session) = sessions.get_mut(&sid) {
                        let cmd_with_newline = format!("{}\n", cmd_str);
                        let _ = session.writer.write_all(cmd_with_newline.as_bytes());
                    }
                }
            });
        }

        // Drop the app_handle clone — exit events are emitted via kill()
        drop(app_handle);

        // Store session
        let session = PtySession {
            writer,
            master: pair.master,
            child,
        };

        self.sessions
            .lock()
            .unwrap()
            .insert(session_id, session);

        Ok(())
    }

    pub fn write(&self, session_id: &str, data: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(session_id) {
            session.writer.write_all(data.as_bytes())?;
        }
        Ok(())
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(session_id) {
            session.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
        }
        Ok(())
    }

    pub fn kill(&self, session_id: &str, app_handle: &AppHandle) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(mut session) = sessions.remove(session_id) {
            let _ = session.child.kill();
            let exit_code = session
                .child
                .wait()
                .map(|s| {
                    if s.success() {
                        0
                    } else {
                        1
                    }
                })
                .unwrap_or(1);

            let _ = app_handle.emit(
                "terminal:exit",
                TerminalExitEvent {
                    session_id: session_id.to_string(),
                    exit_code,
                    signal: None,
                },
            );
        }
        Ok(())
    }
}
