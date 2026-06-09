//! "Server mode" for the desktop app: run the embedded Workbench control-plane
//! server so other devices (phone, another machine) can list/create worktrees
//! and spawn `claude remote-control` sessions on this machine.
//!
//! There are two independent server slots:
//!
//! * **loopback** — always-on, 127.0.0.1 on an ephemeral port, started at
//!   boot before the webview mounts. This is what desktop xterm panes attach
//!   to over WebSocket. It is never stopped by `stop_server`.
//!
//! * **lan** — opt-in "server mode" (0.0.0.0 or user-chosen bind), toggled
//!   by `start_server`/`stop_server` from the settings UI.
//!
//! The shared spawn+double-checked-lock logic lives in `ensure_started`.

use std::sync::Mutex;

use serde::Serialize;
use workbench_server::ServerHandle;

/// Managed Tauri state holding the two independent embedded server handles.
#[derive(Default)]
pub struct ServerControl {
    /// Always-on loopback server (127.0.0.1, ephemeral port). Booted before
    /// the webview and never torn down during normal operation.
    loopback: Mutex<Option<ServerHandle>>,
    /// Optional LAN server (0.0.0.0 or user-chosen bind), toggled via
    /// `start_server`/`stop_server` from the server-mode settings UI.
    lan: Mutex<Option<ServerHandle>>,
}

impl ServerControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a `ServerHandle` in the loopback slot. Called from `lib.rs` setup
    /// after `spawn_embedded` succeeds, before the webview is shown.
    pub fn set_loopback(&self, handle: ServerHandle) {
        let mut guard = self.loopback.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(handle);
    }
}

/// Status returned by server commands and `terminal_server_status`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub address: Option<String>,
    /// Token required to authenticate against this server (if any). Always
    /// `None` for the loopback server (no auth needed on 127.0.0.1).
    pub token: Option<String>,
}

// ---------------------------------------------------------------------------
// Internal helper — DRY spawn + double-checked-lock used by both boot and
// `start_server`.
// ---------------------------------------------------------------------------

/// Ensure a server is running in the given `slot`.  Returns the current
/// address on success (whether a new server was started or one was already
/// running).  If a concurrent call wins the race, the freshly-spawned server
/// is stopped to avoid orphans.
///
/// # Arguments
/// * `slot`  – the `Mutex<Option<ServerHandle>>` to populate
/// * `bind`  – bind address string (e.g. `"127.0.0.1"` or `"0.0.0.0"`)
/// * `port`  – 0 for an ephemeral port
/// * `token` – optional bearer token
async fn ensure_started(
    slot: &Mutex<Option<ServerHandle>>,
    bind: &str,
    port: u16,
    token: Option<String>,
) -> Result<String, String> {
    // Fast-path: already running.
    {
        let guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = guard.as_ref() {
            return Ok(handle.addr().to_string());
        }
    }

    let handle = workbench_server::spawn_embedded(bind, port, token)
        .await
        .map_err(|e| e.to_string())?;
    let address = handle.addr().to_string();

    // Re-check after the await: a concurrent call may have won the race.
    // If so, stop this freshly-spawned server so it isn't orphaned.
    let existing_addr = {
        let guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|h| h.addr().to_string())
    };
    if let Some(addr) = existing_addr {
        handle.stop().await;
        return Ok(addr);
    }

    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(handle);

    Ok(address)
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Start the LAN server (opt-in server mode). Has no effect on the loopback
/// server.
#[tauri::command]
pub async fn start_server(
    state: tauri::State<'_, ServerControl>,
    bind: Option<String>,
    port: u16,
    token: Option<String>,
) -> Result<ServerStatus, String> {
    let bind = bind.unwrap_or_else(|| "0.0.0.0".to_string());
    let address = ensure_started(&state.lan, &bind, port, token).await?;
    Ok(ServerStatus {
        running: true,
        address: Some(address),
        token: None,
    })
}

/// Stop the LAN server. Has no effect on the loopback server.
#[tauri::command]
pub async fn stop_server(state: tauri::State<'_, ServerControl>) -> Result<ServerStatus, String> {
    let handle = {
        let mut guard = state.lan.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(handle) = handle {
        handle.stop().await;
    }
    Ok(ServerStatus {
        running: false,
        address: None,
        token: None,
    })
}

/// Status of the LAN server (used by the server-mode settings UI).
#[tauri::command]
pub fn server_status(state: tauri::State<'_, ServerControl>) -> ServerStatus {
    let guard = state.lan.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(handle) => ServerStatus {
            running: true,
            address: Some(handle.addr().to_string()),
            token: None,
        },
        None => ServerStatus {
            running: false,
            address: None,
            token: None,
        },
    }
}

/// Status of the always-on loopback server. Used by the frontend's terminal
/// layer (`terminal.ts`) to learn the `ws://127.0.0.1:<port>` base URL it
/// must connect to for xterm WebSocket sessions.
#[tauri::command]
pub fn terminal_server_status(state: tauri::State<'_, ServerControl>) -> ServerStatus {
    let guard = state.loopback.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(handle) => ServerStatus {
            running: true,
            address: Some(handle.addr().to_string()),
            token: None,
        },
        None => ServerStatus {
            running: false,
            address: None,
            token: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;

    /// Build a headless mock Tauri app (no webview) holding the ServerControl state.
    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(ServerControl::new())
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    // -----------------------------------------------------------------------
    // LAN slot: start / status / stop cycle (preserves old behaviour)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn lan_start_status_stop_cycle() {
        let app = mock_app();

        // Initially stopped (both slots).
        assert!(!server_status(app.state()).running);
        assert!(!terminal_server_status(app.state()).running);

        // Start on an ephemeral port (bind 127.0.0.1 so the test never
        // exposes a publicly-routable port).
        let started = start_server(app.state(), Some("127.0.0.1".to_string()), 0, None)
            .await
            .expect("start_server");
        assert!(started.running);
        assert!(started.address.is_some());

        // LAN status reflects the running server.
        let status = server_status(app.state());
        assert!(status.running);
        assert_eq!(status.address, started.address);

        // Starting again while running is a no-op that returns the same address
        // (exercises the double-checked-lock guard, not a second bind).
        let again = start_server(app.state(), None, 0, None)
            .await
            .expect("second start is idempotent");
        assert_eq!(again.address, started.address);

        // Stop, and confirm LAN status goes back to stopped.
        let stopped = stop_server(app.state()).await.expect("stop_server");
        assert!(!stopped.running);
        assert!(!server_status(app.state()).running);

        // Stopping the LAN server must NOT affect the loopback slot.
        assert!(!terminal_server_status(app.state()).running);
    }

    // -----------------------------------------------------------------------
    // Loopback slot: boot path + terminal_server_status
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn loopback_boot_and_terminal_status() {
        let app = mock_app();

        // Simulate what lib.rs setup() does: spawn the loopback server and
        // store it via set_loopback().
        let sc: tauri::State<'_, ServerControl> = app.state();
        let handle = workbench_server::spawn_embedded("127.0.0.1", 0, None)
            .await
            .expect("spawn loopback");
        let addr = handle.addr().to_string();
        sc.set_loopback(handle);

        // terminal_server_status should now report running with the address.
        let ts = terminal_server_status(app.state());
        assert!(ts.running);
        assert_eq!(ts.address.as_deref(), Some(addr.as_str()));
        assert!(ts.token.is_none());

        // LAN status must remain unaffected.
        assert!(!server_status(app.state()).running);
    }

    // -----------------------------------------------------------------------
    // Independence: stopping LAN does not touch loopback, and vice versa
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn slots_are_independent() {
        let app = mock_app();

        // Boot loopback.
        {
            let sc: tauri::State<'_, ServerControl> = app.state();
            let handle = workbench_server::spawn_embedded("127.0.0.1", 0, None)
                .await
                .expect("spawn loopback");
            sc.set_loopback(handle);
        }

        // Start LAN server.
        let lan = start_server(app.state(), Some("127.0.0.1".to_string()), 0, None)
            .await
            .expect("start LAN");
        let lan_addr = lan.address.clone().unwrap();

        // Loopback still running; addresses differ (different ports).
        let lb = terminal_server_status(app.state());
        assert!(lb.running);
        assert_ne!(lb.address.as_deref(), Some(lan_addr.as_str()));

        // Stop the LAN server.
        let stopped = stop_server(app.state()).await.expect("stop LAN");
        assert!(!stopped.running);

        // Loopback must still be running.
        assert!(terminal_server_status(app.state()).running);
        // LAN must be gone.
        assert!(!server_status(app.state()).running);
    }
}
