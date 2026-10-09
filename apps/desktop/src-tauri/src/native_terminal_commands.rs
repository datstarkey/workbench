//! Tauri command handlers for native macOS terminal (SwiftTerm) views.
//!
//! All commands in this file are gated behind `#[cfg(target_os = "macos")]`.
//! Non-macOS stubs for `is_native_terminal_available` live in `commands.rs`.
//!
//! A native pane's PTY is a server terminal, created exactly as
//! `POST /remote/terminals` creates one (`terminal::create_from_body`); the
//! view only shows it. Create, resize and kill block (openpty + spawn,
//! `DispatchQueue.main.sync` into SwiftTerm, waiting for the shell to exit),
//! so they run on `crate::blocking`; write only queues input.

#![cfg(target_os = "macos")]

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
) -> Result<Option<String>, String> {
    // A raw pointer isn't `Send`; the view outlives the call (it's the window's).
    let ns_view = window.ns_view().map_err(|e| e.to_string())? as usize;
    crate::blocking(move || {
        let managers = app_handle.state::<ServerControl>().managers();
        // `project_path` is the pane's cwd: a worktree when it isn't the root.
        let root = project_root.unwrap_or_else(|| project_path.clone());
        let worktree_path = (root != project_path).then_some(project_path);
        // Picked here, as a server terminal create picks it (`for_launch`).
        let claude_account_id = match claude_session.as_ref() {
            Some(session) => workbench_core::claude_accounts::for_launch_saved(
                claude_account_id.as_deref(),
                &root,
                Some(&session.id),
            )
            .map_err(|e| e.to_string())?,
            None => claude_account_id,
        };
        let body = workbench_server::terminal::CreateTerminalBody {
            project_path: root,
            worktree_path,
            name: None,
            command: startup_command,
            claude_session,
            codex_session: None,
            cols: 80,
            rows: 24,
            pane_id: Some(session_id.clone()),
            shell: Some(shell),
            claude_account_id,
            native: true,
        };
        let meta = workbench_server::terminal::create_from_body(
            &managers.terminals,
            &managers.agents,
            body,
        )
        .map_err(|e| e.to_string())?;
        let attached = app_handle.state::<NativeTerminalManager>().attach(
            session_id,
            meta.id.clone(),
            managers.terminals.clone(),
            (x, y, width, height, font_size),
            ns_view as *mut std::ffi::c_void,
            app_handle.clone(),
        );
        if let Err(e) = attached {
            managers.terminals.kill(&meta.id);
            return Err(e.to_string());
        }
        Ok(meta.notice)
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

/// Close a native pane: an End, as closing any pane is, so a chat its
/// `claude` hosts ends on every device.
#[tauri::command]
pub async fn kill_native_terminal(
    session_id: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    crate::blocking(move || {
        let terminal = app_handle
            .state::<NativeTerminalManager>()
            .kill(&session_id);
        if let Some(id) = terminal {
            let managers = app_handle.state::<ServerControl>().managers();
            managers.agents.end_terminal(&id);
            managers.terminals.kill_and_wait(&id);
        }
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
