//! Tauri command handlers for native macOS terminal (SwiftTerm) views.
//!
//! All commands in this file are gated behind `#[cfg(target_os = "macos")]`.
//! Non-macOS stubs for `is_native_terminal_available` live in `commands.rs`.
//!
//! A native pane's PTY is a server terminal the workspace service starts and
//! ends; the view only shows it. Attach, resize and detach block
//! (`DispatchQueue.main.sync` into SwiftTerm), so they run on
//! `crate::blocking`; write only queues input.

#![cfg(target_os = "macos")]

use crate::native_terminal::NativeTerminalManager;
use crate::server_control::ServerControl;
use tauri::Manager;

/// Show server terminal `terminal_id` (a native pane's, started by the
/// workspace service) in a SwiftTerm view keyed by the pane id.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn attach_native_terminal(
    session_id: String,
    terminal_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    font_size: f64,
    window: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // A raw pointer isn't `Send`; the view outlives the call (it's the window's).
    let ns_view = window.ns_view().map_err(|e| e.to_string())? as usize;
    crate::blocking(move || {
        let terminals = app_handle.state::<ServerControl>().managers().terminals;
        app_handle
            .state::<NativeTerminalManager>()
            .attach(
                session_id,
                terminal_id,
                terminals,
                (x, y, width, height, font_size),
                ns_view as *mut std::ffi::c_void,
                app_handle.clone(),
            )
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn resize_native_terminal(
    session_id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    app_handle
        .state::<NativeTerminalManager>()
        .request_resize(&session_id, (x, y, width, height))
        .map_err(|e| e.to_string())?;
    crate::blocking(move || {
        let terminals = app_handle.state::<ServerControl>().managers().terminals;
        app_handle
            .state::<NativeTerminalManager>()
            .resize(&session_id, &terminals)
            .map_err(|e| e.to_string())
    })
    .await
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

/// Remove a native pane's view; its terminal runs on until the pane closes.
#[tauri::command]
pub async fn detach_native_terminal(
    session_id: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    crate::blocking(move || {
        app_handle
            .state::<NativeTerminalManager>()
            .detach(&session_id);
        Ok(())
    })
    .await
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
