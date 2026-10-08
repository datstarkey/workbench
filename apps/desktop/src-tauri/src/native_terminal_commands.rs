//! Tauri command handlers for native macOS terminal (SwiftTerm).
//!
//! All commands in this file are gated behind `#[cfg(target_os = "macos")]`.
//! Non-macOS stubs for `is_native_terminal_available` live in `commands.rs`.
//!
//! Create, resize and kill block (openpty + spawn, `DispatchQueue.main.sync`
//! into SwiftTerm, waiting for the shell to exit), so they run on
//! `crate::blocking`; write only queues input.

#![cfg(target_os = "macos")]

use crate::hook_bridge::HookBridgeState;
use crate::native_terminal::NativeTerminalManager;
use crate::server_control::ServerControl;
use tauri::Manager;

#[allow(clippy::too_many_arguments)]
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
    claude_session: Option<workbench_core::claude_launch::ClaudeSessionLaunch>,
    claude_account_id: Option<String>,
    project_root: Option<String>,
    window: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
    hook_bridge: tauri::State<'_, HookBridgeState>,
) -> Result<Option<String>, String> {
    // A raw pointer isn't `Send`; the view outlives the call (it's the window's).
    let ns_view = window.ns_view().map_err(|e| e.to_string())? as usize;
    let hook_socket = hook_bridge.socket_path().map(str::to_string);
    crate::blocking(move || {
        let server = app_handle.state::<ServerControl>();
        let manager = app_handle.state::<NativeTerminalManager>();
        create(
            session_id,
            project_path,
            shell,
            (x, y, width, height, font_size),
            startup_command,
            claude_session,
            claude_account_id,
            project_root,
            hook_socket,
            ns_view as *mut std::ffi::c_void,
            &manager,
            &server,
            app_handle.clone(),
        )
    })
    .await
}

#[allow(clippy::too_many_arguments)]
fn create(
    session_id: String,
    project_path: String,
    shell: String,
    (x, y, width, height, font_size): (f64, f64, f64, f64, f64),
    startup_command: Option<String>,
    mut claude_session: Option<workbench_core::claude_launch::ClaudeSessionLaunch>,
    claude_account_id: Option<String>,
    project_root: Option<String>,
    hook_socket: Option<String>,
    ns_view: *mut std::ffi::c_void,
    manager: &NativeTerminalManager,
    server: &ServerControl,
    app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    let claude_config_dir = crate::claude_accounts::resolve_saved(claude_account_id.as_deref())
        .map_err(|e| e.to_string())?;
    // Decided and built here as a server terminal's is (`terminal::create_from_body`),
    // so the sandbox wrapper fails closed.
    if let Some(session) = claude_session.as_mut() {
        session.resume = workbench_server::agent::claude_history_exists(
            claude_config_dir.as_deref(),
            &session.id,
        );
    }
    let notice = claude_session
        .as_ref()
        .and_then(workbench_core::claude_launch::prompt_notice);
    let startup_command =
        workbench_core::claude_launch::startup_command(startup_command, claude_session.as_ref())
            .map_err(|e| e.to_string())?;
    let mod_env = server.grant_native_terminal(
        &session_id,
        project_root.as_deref().unwrap_or(&project_path),
        &project_path,
        claude_account_id,
        hook_socket.clone(),
    );

    let spawned = manager.spawn(
        session_id.clone(),
        project_path,
        shell,
        x,
        y,
        width,
        height,
        font_size,
        startup_command,
        hook_socket,
        claude_config_dir,
        mod_env,
        ns_view,
        app_handle,
    );
    if spawned.is_err() {
        server.revoke_native_terminal(&session_id);
    }
    spawned.map(|()| notice).map_err(|e| e.to_string())
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
        app_handle
            .state::<NativeTerminalManager>()
            .resize(&session_id)
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

#[tauri::command]
pub async fn kill_native_terminal(
    session_id: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    crate::blocking(move || {
        app_handle
            .state::<ServerControl>()
            .revoke_native_terminal(&session_id);
        app_handle
            .state::<NativeTerminalManager>()
            .kill(&session_id)
            .map_err(|e| e.to_string())
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
