//! Native macOS terminal support using SwiftTerm via FFI.
//!
//! Data flows directly from the PTY reader thread to SwiftTerm via
//! `swift_term_feed()`, bypassing the frontend WebView for terminal I/O.
//! Activity events (`terminal:activity`) and exit events (`terminal:exit`)
//! are still emitted to the frontend via Tauri events.

#![cfg(target_os = "macos")]

use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::io::{Read, Write};
use std::os::raw::c_char;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};

use crate::types::{TerminalActivityEvent, TerminalDataEvent, TerminalExitEvent};

const PTY_READ_BUFFER_SIZE: usize = 32768;
const STARTUP_COMMAND_DELAY_MS: u64 = 300;
const TERMINAL_QUIET_THRESHOLD_MS: u64 = 1000;

// ---------------------------------------------------------------------------
// FFI declarations for SwiftTermBridge
// ---------------------------------------------------------------------------

type SwiftTermInputCallback = extern "C" fn(*mut c_void, *const c_void, usize);
type SwiftTermActivityCallback = extern "C" fn(*mut c_void, bool);

#[allow(dead_code)]
extern "C" {
    fn swift_term_create(
        session_id: *const c_char,
        parent_ns_view: *mut c_void,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        font_size: f64,
        font_family: *const c_char,
        input_callback: SwiftTermInputCallback,
        activity_callback: SwiftTermActivityCallback,
        callback_context: *mut c_void,
    ) -> bool;

    fn swift_term_feed(session_id: *const c_char, data: *const c_void, len: usize);

    fn swift_term_resize(session_id: *const c_char, x: f64, y: f64, width: f64, height: f64);

    fn swift_term_get_size(
        session_id: *const c_char,
        out_cols: *mut u16,
        out_rows: *mut u16,
    );

    fn swift_term_set_visible(session_id: *const c_char, visible: bool);

    fn swift_term_write(session_id: *const c_char, text: *const c_char);

    fn swift_term_destroy(session_id: *const c_char);
}

// ---------------------------------------------------------------------------
// Callback context — heap-allocated, passed to Swift as a raw pointer
// ---------------------------------------------------------------------------

struct CallbackContext {
    session_id: String,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    app_handle: AppHandle,
}

/// Called by SwiftTerm when the user types input (keystrokes).
/// Writes the data directly to the PTY writer.
extern "C" fn input_callback(context: *mut c_void, data: *const c_void, len: usize) {
    if context.is_null() || data.is_null() || len == 0 {
        return;
    }
    let ctx = unsafe { &*(context as *const CallbackContext) };
    let bytes = unsafe { std::slice::from_raw_parts(data as *const u8, len) };
    if let Ok(mut writer) = ctx.writer.lock() {
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }
}

/// Called by SwiftTerm when terminal activity state changes.
/// Emits `terminal:activity` events so the frontend can track quiescence.
extern "C" fn activity_callback(context: *mut c_void, active: bool) {
    if context.is_null() {
        return;
    }
    let ctx = unsafe { &*(context as *const CallbackContext) };
    let _ = ctx.app_handle.emit(
        "terminal:activity",
        TerminalActivityEvent {
            session_id: ctx.session_id.clone(),
            active,
        },
    );
}

// ---------------------------------------------------------------------------
// Session types
// ---------------------------------------------------------------------------

struct NativeSession {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    #[allow(dead_code)]
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Raw pointer to the heap-allocated CallbackContext.
    /// Freed in `kill()` via `Box::from_raw()`.
    callback_context_ptr: *mut c_void,
    session_id_cstr: CString,
}

// SAFETY: The raw pointer is only dereferenced on the main thread (FFI calls)
// and in the callbacks which hold their own synchronization via Arc<Mutex>.
unsafe impl Send for NativeSession {}
unsafe impl Sync for NativeSession {}

type SessionMap = Arc<Mutex<HashMap<String, Arc<Mutex<NativeSession>>>>>;

// ---------------------------------------------------------------------------
// NativeTerminalManager
// ---------------------------------------------------------------------------

pub struct NativeTerminalManager {
    sessions: SessionMap,
}

impl NativeTerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a reference to a session by ID. Locks the map only briefly.
    fn get_session(&self, session_id: &str) -> Option<Arc<Mutex<NativeSession>>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
    }

    /// Remove a session from the map. Returns the session if it existed.
    fn remove_session(
        sessions: &SessionMap,
        session_id: &str,
    ) -> Option<Arc<Mutex<NativeSession>>> {
        sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id)
    }

    fn default_shell() -> String {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &self,
        session_id: String,
        project_path: String,
        shell: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        font_size: f64,
        startup_command: Option<String>,
        hook_socket_path: Option<String>,
        ns_view_ptr: *mut c_void,
        app_handle: AppHandle,
    ) -> Result<()> {
        let pty_system = native_pty_system();

        // Start with a default size — we'll resize after SwiftTerm reports actual
        // cols/rows based on the view frame.
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(size).context("Failed to open PTY")?;

        let shell_path = if shell.is_empty() {
            Self::default_shell()
        } else {
            shell
        };

        let mut cmd = CommandBuilder::new(&shell_path);
        cmd.arg("-l");
        cmd.cwd(&project_path);

        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
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
        cmd.env("WORKBENCH_PANE_ID", session_id.clone());
        if let Some(socket_path) = hook_socket_path {
            cmd.env("WORKBENCH_HOOK_SOCKET", socket_path);
        }

        // Shell integration (OSC 133) — inject ZDOTDIR for zsh
        if startup_command.is_none() && shell_path.contains("zsh") {
            if let Ok(zsh_dir) = crate::shell_integration::ensure_shell_integration_dir() {
                if let Ok(orig) = std::env::var("ZDOTDIR") {
                    cmd.env("WORKBENCH_ORIG_ZDOTDIR", orig);
                } else if let Ok(home) = std::env::var("HOME") {
                    cmd.env("WORKBENCH_ORIG_ZDOTDIR", home);
                }
                cmd.env("ZDOTDIR", zsh_dir.to_string_lossy().as_ref());
            }
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

        let writer = Arc::new(Mutex::new(writer));

        // Create the callback context on the heap
        let ctx = Box::new(CallbackContext {
            session_id: session_id.clone(),
            writer: Arc::clone(&writer),
            app_handle: app_handle.clone(),
        });
        let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

        // Create the SwiftTerm view
        let session_cstr =
            CString::new(session_id.clone()).context("Invalid session_id for CString")?;
        let font_family_cstr =
            CString::new("Menlo").context("Invalid font family for CString")?;

        let created = unsafe {
            swift_term_create(
                session_cstr.as_ptr(),
                ns_view_ptr,
                x,
                y,
                width,
                height,
                font_size,
                font_family_cstr.as_ptr(),
                input_callback,
                activity_callback,
                ctx_ptr,
            )
        };

        if !created {
            // Reclaim the context to avoid a leak
            let _ = unsafe { Box::from_raw(ctx_ptr as *mut CallbackContext) };
            return Err(anyhow!("swift_term_create failed for session {session_id}"));
        }

        // Resize the PTY to match SwiftTerm's actual grid size
        let mut cols: u16 = 80;
        let mut rows: u16 = 24;
        unsafe {
            swift_term_get_size(session_cstr.as_ptr(), &mut cols, &mut rows);
        }
        if cols > 0 && rows > 0 {
            let _ = pair.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }

        let session = Arc::new(Mutex::new(NativeSession {
            writer,
            master: pair.master,
            child,
            callback_context_ptr: ctx_ptr,
            session_id_cstr: session_cstr.clone(),
        }));

        // Insert into map before spawning threads
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.clone(), Arc::clone(&session));

        // ── Activity tracking thread ───────────────────────────────────
        let activity_sid = session_id.clone();
        let activity_handle = app_handle.clone();
        let (activity_tx, activity_rx) = std::sync::mpsc::channel::<()>();
        let quiet_window = Duration::from_millis(TERMINAL_QUIET_THRESHOLD_MS);

        std::thread::spawn(move || {
            let mut active = false;
            loop {
                let signal = match activity_rx.recv_timeout(quiet_window) {
                    Ok(()) => true,     // data received
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        // Emit final inactive if needed
                        if active {
                            let _ = activity_handle.emit(
                                "terminal:activity",
                                TerminalActivityEvent {
                                    session_id: activity_sid.clone(),
                                    active: false,
                                },
                            );
                        }
                        break;
                    }
                };

                let (was_active, is_active) = (active, signal);
                match (was_active, is_active) {
                    (false, true) => {
                        active = true;
                        let _ = activity_handle.emit(
                            "terminal:activity",
                            TerminalActivityEvent {
                                session_id: activity_sid.clone(),
                                active: true,
                            },
                        );
                    }
                    (true, false) => {
                        active = false;
                        let _ = activity_handle.emit(
                            "terminal:activity",
                            TerminalActivityEvent {
                                session_id: activity_sid.clone(),
                                active: false,
                            },
                        );
                    }
                    _ => {}
                }
            }
        });

        // ── Reader thread — feeds data to SwiftTerm via FFI ────────────
        let reader_session_cstr = session_cstr.clone();
        let sessions_for_cleanup = Arc::clone(&self.sessions);
        let session_for_cleanup = Arc::clone(&session);
        let sid = session_id.clone();
        let handle = app_handle;

        std::thread::spawn(move || {
            let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = activity_tx.send(());
                        unsafe {
                            swift_term_feed(
                                reader_session_cstr.as_ptr(),
                                buf.as_ptr() as *const c_void,
                                n,
                            );
                        }
                        // Also emit terminal:data so ClaudeSessionStore can
                        // track session state (awaitingInput, active output).
                        if let Ok(text) = std::str::from_utf8(&buf[..n]) {
                            let _ = handle.emit(
                                "terminal:data",
                                TerminalDataEvent {
                                    session_id: sid.clone(),
                                    data: text.to_string(),
                                },
                            );
                        }
                    }
                    Err(_) => break,
                }
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

            // Destroy the SwiftTerm view
            unsafe {
                swift_term_destroy(reader_session_cstr.as_ptr());
            }

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
                std::thread::sleep(Duration::from_millis(STARTUP_COMMAND_DELAY_MS));
                if let Ok(sess) = session_ref.lock() {
                    if let Ok(mut w) = sess.writer.lock() {
                        let cmd_with_newline = format!("{}\n", cmd_str);
                        let _ = w.write_all(cmd_with_newline.as_bytes());
                    }
                }
            });
        }

        Ok(())
    }

    pub fn resize(
        &self,
        session_id: &str,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;

        let sess = session.lock().unwrap_or_else(|e| e.into_inner());

        // Update the SwiftTerm view frame
        unsafe {
            swift_term_resize(sess.session_id_cstr.as_ptr(), x, y, width, height);
        }

        // Read back the new grid dimensions
        let mut cols: u16 = 0;
        let mut rows: u16 = 0;
        unsafe {
            swift_term_get_size(sess.session_id_cstr.as_ptr(), &mut cols, &mut rows);
        }

        if cols > 0 && rows > 0 {
            sess.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
        }

        Ok(())
    }

    pub fn set_visible(&self, session_id: &str, visible: bool) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;

        let sess = session.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            swift_term_set_visible(sess.session_id_cstr.as_ptr(), visible);
        }
        Ok(())
    }

    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;

        let sess = session.lock().unwrap_or_else(|e| e.into_inner());
        let mut writer = sess.writer.lock().unwrap_or_else(|e| e.into_inner());
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn kill(&self, session_id: &str) -> Result<()> {
        let session = match Self::remove_session(&self.sessions, session_id) {
            Some(s) => s,
            None => return Ok(()), // already cleaned up by reader thread
        };

        let mut sess = session.lock().unwrap_or_else(|e| e.into_inner());

        // Destroy the SwiftTerm view
        unsafe {
            swift_term_destroy(sess.session_id_cstr.as_ptr());
        }

        // Free the callback context
        if !sess.callback_context_ptr.is_null() {
            let _ = unsafe { Box::from_raw(sess.callback_context_ptr as *mut CallbackContext) };
            sess.callback_context_ptr = std::ptr::null_mut();
        }

        // Kill the child process
        let _ = sess.child.kill();
        let _ = sess.child.wait();

        Ok(())
    }
}
