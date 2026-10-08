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
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter, Manager};

use crate::types::{TerminalActivityEvent, TerminalDataEvent, TerminalExitEvent};

const PTY_READ_BUFFER_SIZE: usize = 32768;
const STARTUP_COMMAND_DELAY_MS: u64 = 300;
const TERMINAL_QUIET_THRESHOLD_MS: u64 = 1000;
/// Output is handed to SwiftTerm (one main-thread dispatch) and the webview (one
/// `terminal:data` event) at most once per frame, so a flood of output can't
/// swamp either; a lone chunk after a quiet frame goes out at once.
const OUTPUT_FRAME: Duration = Duration::from_millis(33);
/// A frame never holds more than this, so a steady flood still flushes.
const OUTPUT_FRAME_MAX: usize = 256 * 1024;

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

    fn swift_term_get_size(session_id: *const c_char, out_cols: *mut u16, out_rows: *mut u16);

    fn swift_term_set_visible(session_id: *const c_char, visible: bool);

    fn swift_term_write(session_id: *const c_char, text: *const c_char);

    fn swift_term_destroy(session_id: *const c_char);
}

// ---------------------------------------------------------------------------
// Callback context — heap-allocated, passed to Swift as a raw pointer
// ---------------------------------------------------------------------------

struct CallbackContext {
    session_id: String,
    input: Sender<Vec<u8>>,
    app_handle: AppHandle,
}

/// Called by SwiftTerm, on the main thread, when the user types or pastes.
/// Only queues the bytes: a PTY write blocks once the shell's input queue is
/// full, which here would freeze the whole window (see `spawn_writer`).
extern "C" fn input_callback(context: *mut c_void, data: *const c_void, len: usize) {
    if context.is_null() || data.is_null() || len == 0 {
        return;
    }
    let ctx = unsafe { &*(context as *const CallbackContext) };
    let bytes = unsafe { std::slice::from_raw_parts(data as *const u8, len) };
    let _ = ctx.input.send(bytes.to_vec());
}

/// Write a session's input on its own thread, in order. The queue is unbounded
/// because its producers (keystrokes and pastes on the main thread, the write
/// command) must never block, and what a person types is bounded anyway. An
/// empty chunk is the stop signal (the shell exited); dropping every sender
/// (`kill`) also ends it.
fn spawn_writer(mut writer: Box<dyn Write + Send>, input: Receiver<Vec<u8>>) {
    std::thread::spawn(move || {
        while let Ok(bytes) = input.recv() {
            if bytes.is_empty()
                || writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
            {
                break;
            }
        }
    });
}

/// Hand output to SwiftTerm and the webview a frame at a time (`OUTPUT_FRAME`).
/// Returns once the reader drops its sender, after flushing what's left.
fn pump_output(
    output: Receiver<Vec<u8>>,
    session_cstr: &CString,
    session_id: &str,
    activity: &Sender<()>,
    handle: &AppHandle,
) {
    let mut last_flush = Instant::now() - OUTPUT_FRAME;
    while let Ok(mut frame) = output.recv() {
        let deadline = last_flush + OUTPUT_FRAME;
        let mut open = true;
        while frame.len() < OUTPUT_FRAME_MAX {
            match output.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(more) => frame.extend_from_slice(&more),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    open = false;
                    break;
                }
            }
        }
        let _ = activity.send(());
        unsafe {
            swift_term_feed(
                session_cstr.as_ptr(),
                frame.as_ptr() as *const c_void,
                frame.len(),
            );
        }
        // ClaudeSessionStore reads the text to tell a Codex pane's real output
        // from echo and redraws.
        let _ = handle.emit(
            "terminal:data",
            TerminalDataEvent {
                session_id: session_id.to_string(),
                data: String::from_utf8_lossy(&frame).into_owned(),
            },
        );
        last_flush = Instant::now();
        if !open {
            break;
        }
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
    input: Sender<Vec<u8>>,
    #[allow(dead_code)]
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Raw pointer to the heap-allocated CallbackContext.
    /// Freed in `kill()` via `Box::from_raw()`.
    callback_context_ptr: *mut c_void,
    session_id_cstr: CString,
}

// SAFETY: The raw pointer is only dereferenced on the main thread (FFI calls)
// and in the callbacks, which only touch `Send + Sync` fields.
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
        claude_config_dir: Option<std::path::PathBuf>,
        mod_env: Vec<(&'static str, String)>,
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
            crate::shell::default_shell()
        } else {
            shell
        };

        let mut cmd = CommandBuilder::new(&shell_path);
        for arg in crate::shell::login_args() {
            cmd.arg(arg);
        }
        cmd.cwd(&project_path);

        for (key, val) in crate::shell::inherited_env() {
            cmd.env(key, val);
        }
        cmd.env(
            "LANG",
            std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string()),
        );
        cmd.env("WORKBENCH_PANE_ID", session_id.clone());
        if let Some(socket_path) = hook_socket_path {
            cmd.env("WORKBENCH_HOOK_SOCKET", socket_path);
            if let Some(dirs) = workbench_core::claude_plugin::plugin_dirs_env() {
                cmd.env(workbench_core::claude_plugin::PLUGIN_DIRS_ENV, dirs);
            }
        }
        if let Some(dir) = claude_config_dir {
            cmd.env(crate::claude_accounts::CONFIG_DIR_ENV, dir);
        }
        let mod_token = mod_env
            .iter()
            .find(|(key, _)| *key == "WORKBENCH_MOD_TOKEN")
            .map(|(_, token)| token.clone());
        for (key, val) in mod_env {
            cmd.env(key, val);
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

        let (input, input_rx) = mpsc::channel::<Vec<u8>>();
        spawn_writer(
            pair.master
                .take_writer()
                .context("Failed to get PTY writer")?,
            input_rx,
        );

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("Failed to get PTY reader")?;

        // Create the callback context on the heap
        let ctx = Box::new(CallbackContext {
            session_id: session_id.clone(),
            input: input.clone(),
            app_handle: app_handle.clone(),
        });
        let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

        // Create the SwiftTerm view
        let session_cstr =
            CString::new(session_id.clone()).context("Invalid session_id for CString")?;
        let font_family_cstr = CString::new("Menlo").context("Invalid font family for CString")?;

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
            input: input.clone(),
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
                    Ok(()) => true, // data received
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

        // ── Reader thread — output goes to SwiftTerm via `pump_output` ─
        let reader_session_cstr = session_cstr.clone();
        let sessions_for_cleanup = Arc::clone(&self.sessions);
        let session_for_cleanup = Arc::clone(&session);
        let sid = session_id.clone();
        let handle = app_handle;
        let stop_writer = input.clone();

        std::thread::spawn(move || {
            let (output, output_rx) = mpsc::channel::<Vec<u8>>();
            let pump = {
                let (cstr, sid, handle) =
                    (reader_session_cstr.clone(), sid.clone(), handle.clone());
                std::thread::spawn(move || {
                    pump_output(output_rx, &cstr, &sid, &activity_tx, &handle)
                })
            };
            let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let _ = output.send(buf[..n].to_vec());
                    }
                }
            }
            drop(output);
            let _ = pump.join();
            let _ = stop_writer.send(Vec::new());

            // Cleanup: remove session from map and emit exit event.
            Self::remove_session(&sessions_for_cleanup, &sid);
            // A shell that exits on its own never sees `kill_native_terminal`.
            if let Some(token) = &mod_token {
                handle
                    .state::<crate::server_control::ServerControl>()
                    .revoke_native_token(&sid, token);
            }

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
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(STARTUP_COMMAND_DELAY_MS));
                let mut line = Vec::new();
                let _ = crate::shell::submit_line(&mut line, &cmd_str);
                let _ = input.send(line);
            });
        }

        Ok(())
    }

    pub fn resize(&self, session_id: &str, x: f64, y: f64, width: f64, height: f64) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;

        let session_cstr = Self::session_cstr(&session);

        // Not under the session lock: this waits on the main thread
        // (`DispatchQueue.main.sync`), which must never wait on us.
        unsafe {
            swift_term_resize(session_cstr.as_ptr(), x, y, width, height);
        }

        // Read back the new grid dimensions
        let mut cols: u16 = 0;
        let mut rows: u16 = 0;
        unsafe {
            swift_term_get_size(session_cstr.as_ptr(), &mut cols, &mut rows);
        }

        if cols > 0 && rows > 0 {
            let sess = session.lock().unwrap_or_else(|e| e.into_inner());
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

        let session_cstr = Self::session_cstr(&session);
        unsafe {
            swift_term_set_visible(session_cstr.as_ptr(), visible);
        }
        Ok(())
    }

    fn session_cstr(session: &Mutex<NativeSession>) -> CString {
        session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session_id_cstr
            .clone()
    }

    /// Queue input for the session's writer thread; never blocks on the PTY.
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;
        if data.is_empty() {
            return Ok(());
        }
        let input = session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .input
            .clone();
        input
            .send(data.to_vec())
            .map_err(|_| anyhow!("Session has exited: {session_id}"))
    }

    /// Blocking: waits for the shell to exit.
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
