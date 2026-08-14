//! Tauri commands exposing the native macOS notification bridge.

#![cfg(target_os = "macos")]

use crate::native_notifications;

/// Whether the frontend should use the native path. False on unbundled dev builds,
/// where the frontend falls back to the (currently non-delivering) plugin rather than
/// silently dropping notifications.
#[tauri::command]
pub fn is_native_notification_available() -> bool {
    native_notifications::is_available()
}

/// Returns false when the notification was not delivered — unbundled, or the user has
/// not granted authorization.
#[tauri::command]
pub fn send_native_notification(identifier: String, title: String, body: String) -> bool {
    native_notifications::send(&identifier, &title, &body)
}
