//! macOS notifications via `UNUserNotificationCenter` (see `Notifications.swift`).
//!
//! Replaces `tauri-plugin-notification` on macOS, which routes through
//! notify-rust -> mac-notification-sys -> `NSUserNotificationCenter` — deprecated in
//! macOS 11 and no longer delivering. It also swallows the delivery error
//! (`desktop.rs`: `let _ = notification.show()`), so nothing surfaced the failure.

#![cfg(target_os = "macos")]

use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

type NotificationActionCallback = extern "C" fn(*mut c_void, *const c_char);

extern "C" {
    fn wb_notifications_available() -> bool;
    fn wb_notifications_init(callback: NotificationActionCallback, context: *mut c_void) -> bool;
    fn wb_notification_send(
        identifier: *const c_char,
        title: *const c_char,
        body: *const c_char,
    ) -> bool;
}

/// Set once during setup. The Swift delegate outlives any borrow we could pass it, so
/// the click callback reaches the app through here rather than a context pointer.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Emitted when the user clicks a notification; payload is the notification identifier,
/// which the frontend maps back to a pane.
const NOTIFICATION_ACTION_EVENT: &str = "notification:action";

extern "C" fn action_callback(_context: *mut c_void, identifier: *const c_char) {
    if identifier.is_null() {
        return;
    }
    // Safety: Swift passes a NUL-terminated string valid for this call only, and we
    // copy it before returning.
    let id = match unsafe { CStr::from_ptr(identifier) }.to_str() {
        Ok(s) => s.to_owned(),
        Err(e) => {
            log::warn!("notification identifier was not valid UTF-8: {e}");
            return;
        }
    };

    if let Some(app) = APP.get() {
        if let Err(e) = app.emit(NOTIFICATION_ACTION_EVENT, id) {
            log::warn!("failed to emit notification action: {e}");
        }
    }
}

/// Installs the delegate and requests authorization. Call from `setup()` — macOS drops
/// click responses for notifications delivered before the delegate exists.
///
/// Returns false when unavailable, which in practice means an unbundled binary
/// (`tauri dev`): `UNUserNotificationCenter.current()` raises without a bundle id.
pub fn init(app: AppHandle) -> bool {
    let _ = APP.set(app);
    unsafe {
        if !wb_notifications_available() {
            log::info!("native notifications unavailable (unbundled build) — skipping");
            return false;
        }
        wb_notifications_init(action_callback, std::ptr::null_mut())
    }
}

pub fn is_available() -> bool {
    unsafe { wb_notifications_available() }
}

/// Posts a notification. `identifier` is the replace key: reusing one replaces the
/// existing banner instead of stacking, and comes back via `notification:action`.
///
/// False means not delivered — unbundled, or authorization not (yet) granted.
pub fn send(identifier: &str, title: &str, body: &str) -> bool {
    let (Ok(id), Ok(title), Ok(body)) = (
        CString::new(identifier),
        CString::new(title),
        CString::new(body),
    ) else {
        // Interior NUL — the strings are built from project paths and tab labels, so
        // this is malformed input rather than something to panic over.
        log::warn!("notification text contained an interior NUL byte");
        return false;
    };

    unsafe { wb_notification_send(id.as_ptr(), title.as_ptr(), body.as_ptr()) }
}
