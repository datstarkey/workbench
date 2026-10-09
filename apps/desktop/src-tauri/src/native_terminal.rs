//! Native macOS terminal views (SwiftTerm, via FFI) over the server's own
//! terminals. The PTY is a `workbench_server` terminal like an xterm pane's
//! (same launch, cwd allowlist and End-on-close, and other devices can attach
//! it); this view follows its output in-process (`TerminalManager::tap`)
//! straight into `swift_term_feed()`, bypassing the WebView. Activity
//! (`terminal:activity`) and exit (`terminal:exit`) events still go to the
//! frontend.

#![cfg(target_os = "macos")]

use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tauri::{AppHandle, Emitter};
use workbench_server::TerminalManager;

use crate::types::{TerminalActivityEvent, TerminalDataEvent, TerminalExitEvent};

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

/// Hand a view's input to its terminal on its own thread, in order. The queue
/// is unbounded because its producers (keystrokes and pastes on the main
/// thread, the write command) must never block; this thread waits for room in
/// the terminal's queue instead. Ends when every sender is dropped (`kill`).
fn forward_input(terminals: TerminalManager, terminal_id: String, input: Receiver<Vec<u8>>) {
    std::thread::spawn(move || {
        while let Ok(bytes) = input.recv() {
            if !terminals.write(&terminal_id, bytes) {
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
    let mut text_carry = Vec::new();
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
                data: frame_text(&mut text_carry, &frame),
            },
        );
        last_flush = Instant::now();
        if !open {
            break;
        }
    }
}

/// A frame's text, with a character cut off at its end held back in `carry` for
/// the next frame: a split character must not read as U+FFFD output.
fn frame_text(carry: &mut Vec<u8>, frame: &[u8]) -> String {
    let mut bytes = std::mem::take(carry);
    bytes.extend_from_slice(frame);
    if let Err(e) = std::str::from_utf8(&bytes) {
        if e.error_len().is_none() {
            *carry = bytes.split_off(e.valid_up_to());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
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
    /// The server terminal this view shows.
    terminal_id: String,
    input: Sender<Vec<u8>>,
    /// Raw pointer to the heap-allocated CallbackContext.
    /// Freed by `destroy_view()` via `Box::from_raw()`.
    callback_context_ptr: *mut c_void,
    session_id_cstr: CString,
    /// The latest frame asked for (`request_resize`), applied by `resize`.
    frame: Option<Frame>,
}

/// A view frame: x, y, width, height.
pub type Frame = (f64, f64, f64, f64);

impl NativeSession {
    /// Remove the SwiftTerm view, then free the callback context. Called under
    /// the session lock by `kill` and when the terminal exits; the second call
    /// finds nothing to do. `swift_term_destroy` waits for the main thread,
    /// where the callbacks run, so none can use the context once freed.
    fn destroy_view(&mut self) {
        unsafe {
            swift_term_destroy(self.session_id_cstr.as_ptr());
        }
        if !self.callback_context_ptr.is_null() {
            let _ = unsafe { Box::from_raw(self.callback_context_ptr as *mut CallbackContext) };
            self.callback_context_ptr = std::ptr::null_mut();
        }
    }
}

// SAFETY: The raw pointer is only dereferenced in the callbacks, on the main
// thread, and freed only once the view is gone (`destroy_view`).
unsafe impl Send for NativeSession {}
unsafe impl Sync for NativeSession {}

type SessionMap = Arc<Mutex<HashMap<String, Arc<Mutex<NativeSession>>>>>;

// ---------------------------------------------------------------------------
// NativeTerminalManager
// ---------------------------------------------------------------------------

pub struct NativeTerminalManager {
    sessions: SessionMap,
    /// Held while a resize applies, so resizes run one at a time.
    resizing: Mutex<()>,
}

impl NativeTerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            resizing: Mutex::new(()),
        }
    }

    fn get_session(&self, session_id: &str) -> Option<Arc<Mutex<NativeSession>>> {
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
    }

    fn remove_session(
        sessions: &SessionMap,
        session_id: &str,
    ) -> Option<Arc<Mutex<NativeSession>>> {
        sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id)
    }

    /// Show server terminal `terminal_id` in a SwiftTerm view keyed by the pane
    /// id `session_id`, sized to the view.
    pub fn attach(
        &self,
        session_id: String,
        terminal_id: String,
        terminals: TerminalManager,
        (x, y, width, height, font_size): (f64, f64, f64, f64, f64),
        ns_view_ptr: *mut c_void,
        app_handle: AppHandle,
    ) -> Result<()> {
        let tap = terminals
            .tap(&terminal_id)
            .ok_or_else(|| anyhow!("terminal {terminal_id} is gone"))?;
        let (input, input_rx) = mpsc::channel::<Vec<u8>>();
        forward_input(terminals.clone(), terminal_id.clone(), input_rx);

        let ctx = Box::new(CallbackContext {
            session_id: session_id.clone(),
            input: input.clone(),
            app_handle: app_handle.clone(),
        });
        let ctx_ptr = Box::into_raw(ctx) as *mut c_void;
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
            let _ = unsafe { Box::from_raw(ctx_ptr as *mut CallbackContext) };
            return Err(anyhow!("swift_term_create failed for session {session_id}"));
        }

        // The PTY takes SwiftTerm's actual grid size.
        let (mut cols, mut rows) = (80u16, 24u16);
        unsafe {
            swift_term_get_size(session_cstr.as_ptr(), &mut cols, &mut rows);
        }
        if cols > 0 && rows > 0 {
            let _ = terminals.resize(&terminal_id, cols, rows);
        }

        let shown = terminal_id.clone();
        let session = Arc::new(Mutex::new(NativeSession {
            terminal_id,
            input,
            callback_context_ptr: ctx_ptr,
            session_id_cstr: session_cstr.clone(),
            frame: None,
        }));
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id.clone(), Arc::clone(&session));

        let activity = spawn_activity(session_id.clone(), app_handle.clone());
        let (output, output_rx) = mpsc::channel::<Vec<u8>>();
        let pump = {
            let (cstr, sid, handle) = (session_cstr, session_id.clone(), app_handle.clone());
            std::thread::spawn(move || pump_output(output_rx, &cstr, &sid, &activity, &handle))
        };
        let sessions = Arc::clone(&self.sessions);
        tauri::async_runtime::spawn(async move {
            let code = follow(tap, output).await;
            let _ = pump.join();
            // Ended on its own or killed: either way the server lets it go, or
            // it would stay listed (and count toward the cap) with no view.
            terminals.kill(&shown);
            // Still mapped (not killed): take the view down too.
            let exited = {
                let mut map = sessions.lock().unwrap_or_else(|e| e.into_inner());
                let ours = map
                    .get(&session_id)
                    .is_some_and(|s| Arc::ptr_eq(s, &session));
                if ours {
                    map.remove(&session_id)
                } else {
                    None
                }
            };
            if let Some(session) = exited {
                session
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .destroy_view();
            }
            let _ = app_handle.emit(
                "terminal:exit",
                TerminalExitEvent {
                    session_id,
                    exit_code: code.map_or(1, |c| c as i32),
                    signal: None,
                },
            );
        });
        Ok(())
    }

    /// Record the frame a resize asks for, in the order requests arrive; `resize`
    /// then applies whichever is latest.
    pub fn request_resize(&self, session_id: &str, frame: Frame) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;
        session.lock().unwrap_or_else(|e| e.into_inner()).frame = Some(frame);
        Ok(())
    }

    /// Apply the latest requested frame. Blocking (waits on the main thread).
    /// Resizes run on a thread pool in no set order, so each applies the latest
    /// frame rather than its own: whichever runs last leaves the newest size.
    pub fn resize(&self, session_id: &str, terminals: &TerminalManager) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;

        let _resizing = self.resizing.lock().unwrap_or_else(|e| e.into_inner());
        let (session_cstr, frame, terminal_id) = {
            let sess = session.lock().unwrap_or_else(|e| e.into_inner());
            (
                sess.session_id_cstr.clone(),
                sess.frame,
                sess.terminal_id.clone(),
            )
        };
        let Some((x, y, width, height)) = frame else {
            return Ok(());
        };

        // Not under the session lock: this waits on the main thread
        // (`DispatchQueue.main.sync`), which must never wait on us.
        unsafe {
            swift_term_resize(session_cstr.as_ptr(), x, y, width, height);
        }
        let (mut cols, mut rows) = (0u16, 0u16);
        unsafe {
            swift_term_get_size(session_cstr.as_ptr(), &mut cols, &mut rows);
        }
        if cols > 0 && rows > 0 {
            terminals.resize(&terminal_id, cols, rows)?;
        }
        Ok(())
    }

    pub fn set_visible(&self, session_id: &str, visible: bool) -> Result<()> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| anyhow!("Session not found: {session_id}"))?;
        let session_cstr = session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .session_id_cstr
            .clone();
        unsafe {
            swift_term_set_visible(session_cstr.as_ptr(), visible);
        }
        Ok(())
    }

    /// Queue input for the terminal; never blocks.
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

    /// Remove the view; returns the terminal it showed, for the caller to end.
    pub fn kill(&self, session_id: &str) -> Option<String> {
        let session = Self::remove_session(&self.sessions, session_id)?;
        let mut sess = session.lock().unwrap_or_else(|e| e.into_inner());
        sess.destroy_view();
        Some(sess.terminal_id.clone())
    }
}

/// Feed the terminal's scrollback, then its output, to the pump until it ends.
/// Returns its exit code, when it has one.
async fn follow(mut tap: workbench_server::terminal::Tap, output: Sender<Vec<u8>>) -> Option<i64> {
    use tokio::sync::broadcast::error::RecvError;
    if !tap.replay.is_empty() {
        let _ = output.send(std::mem::take(&mut tap.replay));
    }
    while !*tap.done.borrow_and_update() {
        tokio::select! {
            chunk = tap.output.recv() => match chunk {
                Ok(bytes) => {
                    let _ = output.send(bytes);
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
            changed = tap.done.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
    }
    while let Ok(bytes) = tap.output.try_recv() {
        let _ = output.send(bytes);
    }
    tap.exit_code()
}

/// Emits `terminal:activity` when output starts, and again once it has been
/// quiet for `TERMINAL_QUIET_THRESHOLD_MS`. Ends when its sender is dropped.
fn spawn_activity(session_id: String, handle: AppHandle) -> Sender<()> {
    let (tx, rx) = mpsc::channel::<()>();
    let quiet = Duration::from_millis(TERMINAL_QUIET_THRESHOLD_MS);
    std::thread::spawn(move || {
        let emit = |active| {
            let _ = handle.emit(
                "terminal:activity",
                TerminalActivityEvent {
                    session_id: session_id.clone(),
                    active,
                },
            );
        };
        let mut active = false;
        loop {
            let signal = match rx.recv_timeout(quiet) {
                Ok(()) => true,
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => {
                    if active {
                        emit(false);
                    }
                    break;
                }
            };
            if signal != active {
                active = signal;
                emit(active);
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::frame_text;

    #[test]
    fn a_character_split_across_frames_is_held_for_the_next() {
        let bytes = "a─b".as_bytes();
        let mut carry = Vec::new();
        assert_eq!(frame_text(&mut carry, &bytes[..2]), "a");
        assert_eq!(frame_text(&mut carry, &bytes[2..]), "─b");
        assert!(carry.is_empty());
        assert_eq!(frame_text(&mut carry, b"\xff!"), "\u{fffd}!");
    }
}
