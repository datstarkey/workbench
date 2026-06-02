//! "Server mode" for the desktop app: run the embedded Workbench control-plane
//! server so other devices (phone, another machine) can list/create worktrees
//! and spawn `claude remote-control` sessions on this machine.
//!
//! The server is the same one shipped as the standalone `workbench-server`
//! binary (`workbench_server` lib), spawned on Tauri's async runtime.

use std::sync::Mutex;

use serde::Serialize;
use workbench_server::ServerHandle;

/// Managed Tauri state holding the running embedded server, if any.
#[derive(Default)]
pub struct ServerControl {
    handle: Mutex<Option<ServerHandle>>,
}

impl ServerControl {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub address: Option<String>,
}

#[tauri::command]
pub async fn start_server(
    state: tauri::State<'_, ServerControl>,
    bind: Option<String>,
    port: u16,
    token: Option<String>,
) -> Result<ServerStatus, String> {
    {
        let guard = state.handle.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = guard.as_ref() {
            return Ok(ServerStatus {
                running: true,
                address: Some(handle.addr().to_string()),
            });
        }
    }

    let bind = bind.unwrap_or_else(|| "0.0.0.0".to_string());
    let handle = workbench_server::spawn_embedded(&bind, port, token)
        .await
        .map_err(|e| e.to_string())?;
    let address = handle.addr().to_string();

    // Re-check after the await: a concurrent start_server may have won the race.
    // If so, stop this freshly-spawned server so it isn't orphaned (ServerHandle's
    // Drop does NOT stop the server).
    let existing_addr = {
        let guard = state.handle.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|h| h.addr().to_string())
    };
    if let Some(addr) = existing_addr {
        handle.stop().await;
        return Ok(ServerStatus {
            running: true,
            address: Some(addr),
        });
    }

    let mut guard = state.handle.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(handle);

    Ok(ServerStatus {
        running: true,
        address: Some(address),
    })
}

#[tauri::command]
pub async fn stop_server(state: tauri::State<'_, ServerControl>) -> Result<ServerStatus, String> {
    let handle = {
        let mut guard = state.handle.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(handle) = handle {
        handle.stop().await;
    }
    Ok(ServerStatus {
        running: false,
        address: None,
    })
}

#[tauri::command]
pub fn server_status(state: tauri::State<'_, ServerControl>) -> ServerStatus {
    let guard = state.handle.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(handle) => ServerStatus {
            running: true,
            address: Some(handle.addr().to_string()),
        },
        None => ServerStatus {
            running: false,
            address: None,
        },
    }
}
