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

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;

    /// Build a headless mock Tauri app (no webview) holding the ServerControl state,
    /// so the embedded-server commands can be driven exactly as the frontend does.
    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(ServerControl::new())
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    #[tokio::test]
    async fn start_status_stop_cycle() {
        let app = mock_app();

        // Initially stopped.
        assert!(!server_status(app.state()).running);

        // Start on an ephemeral port (bind 127.0.0.1 so the test never exposes a port).
        let started = start_server(app.state(), Some("127.0.0.1".to_string()), 0, None)
            .await
            .expect("start_server");
        assert!(started.running);
        assert!(started.address.is_some());

        // Status reflects the running server.
        let status = server_status(app.state());
        assert!(status.running);
        assert_eq!(status.address, started.address);

        // Starting again while running is a no-op that returns the same address
        // (exercises the double-checked-lock guard, not a second bind).
        let again = start_server(app.state(), None, 0, None)
            .await
            .expect("second start is idempotent");
        assert_eq!(again.address, started.address);

        // Stop, and confirm status goes back to stopped.
        let stopped = stop_server(app.state()).await.expect("stop_server");
        assert!(!stopped.running);
        assert!(!server_status(app.state()).running);
    }
}
