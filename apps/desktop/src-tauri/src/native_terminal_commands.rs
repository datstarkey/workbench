//! Tauri command handlers for native macOS terminal (SwiftTerm).
//!
//! All commands in this file are gated behind `#[cfg(target_os = "macos")]`.
//! Non-macOS stubs for `is_native_terminal_available` live in `commands.rs`.

#![cfg(target_os = "macos")]

use crate::hook_bridge::HookBridgeState;
use crate::native_terminal::NativeTerminalManager;

#[tauri::command]
pub async fn create_native_terminal(
    session_id: String,
    project_path: String,
    shell: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    font_size: f64,
    startup_command: Option<String>,
    manager: tauri::State<'_, NativeTerminalManager>,
    window: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
    hook_bridge: tauri::State<'_, HookBridgeState>,
) -> Result<(), String> {
    let ns_view = window.ns_view().map_err(|e| e.to_string())?;
    let hook_socket = hook_bridge.socket_path().map(str::to_string);

    manager
        .spawn(
            session_id,
            project_path,
            shell,
            x,
            y,
            width,
            height,
            font_size,
            startup_command,
            hook_socket,
            ns_view,
            app_handle,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn resize_native_terminal(
    session_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    manager: tauri::State<'_, NativeTerminalManager>,
) -> Result<(), String> {
    manager
        .resize(&session_id, x, y, width, height)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_native_terminal_visible(
    session_id: String,
    visible: bool,
    manager: tauri::State<'_, NativeTerminalManager>,
) -> Result<(), String> {
    manager
        .set_visible(&session_id, visible)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kill_native_terminal(
    session_id: String,
    manager: tauri::State<'_, NativeTerminalManager>,
) -> Result<(), String> {
    manager.kill(&session_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn write_native_terminal(
    session_id: String,
    data: String,
    manager: tauri::State<'_, NativeTerminalManager>,
) -> Result<(), String> {
    manager
        .write(&session_id, data.as_bytes())
        .map_err(|e| e.to_string())
}
