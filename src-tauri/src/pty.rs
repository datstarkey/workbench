use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};

use crate::types::{TerminalDataEvent, TerminalExitEvent};

const PTY_READ_BUFFER_SIZE: usize = 32768;
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

        // Two-thread output pipeline (same approach as Alacritty / Kitty):
        //   Reader  — drains the PTY as fast as possible (no sleeps, no backpressure)
        //   Emitter — coalesces output and emits to the frontend at a controlled rate
        //
        // During heavy streaming the emitter batches data that accumulated in the
        // channel while it was busy with the previous emit, naturally reducing the
        // number of IPC events and freeing the WebView bridge for input writes.
        // During light activity the emitter fires immediately (no added latency).

        let (data_tx, data_rx) = std::sync::mpsc::channel::<String>();

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
                            if data_tx.send(data.to_string()).is_err() {
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

    /// Send SIGINT to the foreground process (e.g., Claude Code) inside a PTY.
    /// Finds the shell's child processes and signals them directly, bypassing stdin.
    pub fn signal_foreground(&self, session_id: &str) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;
        let sess = session.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(shell_pid) = sess.child.process_id() {
            for child_pid in get_child_pids(shell_pid as i32) {
                unsafe {
                    libc::kill(child_pid, libc::SIGINT);
                }
            }
        }
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

/// List direct child PIDs of a process.
#[cfg(target_os = "macos")]
fn get_child_pids(parent_pid: i32) -> Vec<i32> {
    extern "C" {
        fn proc_listchildpids(
            ppid: libc::pid_t,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    let mut buf = [0i32; 128];
    let bufsize = std::mem::size_of_val(&buf) as libc::c_int;
    let count = unsafe { proc_listchildpids(parent_pid, buf.as_mut_ptr().cast(), bufsize) };
    if count <= 0 {
        return vec![];
    }
    buf[..count as usize].to_vec()
}

#[cfg(target_os = "linux")]
fn get_child_pids(parent_pid: i32) -> Vec<i32> {
    let path = format!("/proc/{parent_pid}/task/{parent_pid}/children");
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}
