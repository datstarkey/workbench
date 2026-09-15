//! "Server mode" for the desktop app: run the embedded Workbench control-plane
//! server so other devices (phone, another machine) can list/create worktrees
//! and spawn `claude remote-control` sessions on this machine.
//!
//! There are two listener slots over ONE set of session managers, so a terminal
//! opened on this machine is visible (and can be taken over) from a phone and
//! vice versa:
//!
//! * **loopback** — always-on, 127.0.0.1 on an ephemeral port, started at
//!   boot before the webview mounts. This is what desktop xterm panes attach
//!   to over WebSocket. It is never stopped by `stop_server`. Its token is
//!   generated per process, held only in memory, and handed to the webview via
//!   `terminal_server_status` — so a local process (e.g. a sandboxed Claude
//!   session) can't drive it.
//!
//! * **lan** — opt-in "server mode" (0.0.0.0 or user-chosen bind), toggled
//!   by `start_server`/`stop_server` from the settings UI. Always requires a
//!   strong token (`workbench_core::token::is_strong`), enforced here.
//!
//! Terminals live in the shared managers, so they survive LAN stop/start.

use std::sync::Mutex;

use serde::Serialize;
use tauri::async_runtime::Mutex as AsyncMutex;
use tauri::Emitter;
use workbench_server::{Managers, ServerHandle};

struct Loopback {
    handle: ServerHandle,
    token: String,
}

/// The running LAN listener and what it was started with, so a token change
/// restarts it (revoking old clients) instead of silently keeping the old token.
struct Lan {
    handle: ServerHandle,
    token: String,
    bind: String,
}

/// Managed Tauri state holding the shared managers and both listener handles.
#[derive(Default)]
pub struct ServerControl {
    managers: Managers,
    loopback: Mutex<Option<Loopback>>,
    /// Async mutex held across start/stop, so concurrent commands serialize.
    lan: AsyncMutex<Option<Lan>>,
}

impl ServerControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start the loopback listener with a fresh in-memory token. Called from
    /// `lib.rs` setup before the webview is shown.
    pub async fn start_loopback(&self) -> anyhow::Result<()> {
        let token = workbench_core::token::generate()?;
        let handle =
            workbench_server::spawn_embedded("127.0.0.1", 0, self.managers.clone(), token.clone())
                .await?;
        let mut guard = self.loopback.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(Loopback { handle, token });
        Ok(())
    }

    /// Ensure the LAN listener runs with `token`. Already running with the same
    /// token → no-op (keeps its address). Running with a different token →
    /// stopped (disconnecting its clients) and restarted on `bind`/`port`.
    async fn ensure_lan(&self, bind: &str, port: u16, token: String) -> Result<String, String> {
        let mut slot = self.lan.lock().await;
        if let Some(lan) = slot.as_ref().filter(|lan| lan.token == token) {
            return Ok(lan.handle.addr().to_string());
        }
        if let Some(old) = slot.take() {
            old.handle.stop().await;
        }
        let handle =
            workbench_server::spawn_embedded(bind, port, self.managers.clone(), token.clone())
                .await
                .map_err(|e| e.to_string())?;
        let address = handle.addr().to_string();
        *slot = Some(Lan {
            handle,
            token,
            bind: bind.to_string(),
        });
        Ok(address)
    }

    /// Switch a running LAN listener to `token` on its current bind and port.
    /// No-op when server mode is off.
    async fn restart_lan_with_token(&self, token: String) -> Result<(), String> {
        let running = self
            .lan
            .lock()
            .await
            .as_ref()
            .map(|lan| (lan.bind.clone(), lan.handle.addr().port()));
        if let Some((bind, port)) = running {
            self.ensure_lan(&bind, port, token).await?;
        }
        Ok(())
    }
}

/// Status returned by server commands and `terminal_server_status`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub address: Option<String>,
    /// Token for the loopback listener (`terminal_server_status` only). The LAN
    /// token lives in settings, so LAN status never echoes it.
    pub token: Option<String>,
}

impl ServerStatus {
    fn stopped() -> Self {
        Self {
            running: false,
            address: None,
            token: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Error surfaced when the LAN server is asked to start without a usable token.
pub const LAN_NEEDS_TOKEN: &str =
    "Server mode requires a token of at least 32 characters. Regenerate the token in settings.";

/// The LAN token to serve with, or `None` when it is missing or too weak.
///
/// Enforced here, in the Rust command, so no frontend caller can start an
/// unauthenticated server. Without a token anyone on the network — and, since
/// sandbox-runtime leaves loopback unfiltered, any sandboxed session on this
/// machine — could spawn an unsandboxed shell.
fn lan_token(token: Option<String>) -> Option<String> {
    token.filter(|t| workbench_core::token::is_strong(t))
}

/// Replace the LAN token: persist ONLY `serverToken` (other settings on disk are
/// untouched, and unsaved form edits never ride along), restart a running LAN
/// listener so clients holding the old token are cut off, and notify windows.
#[tauri::command]
pub async fn rotate_server_token(
    app: tauri::AppHandle,
    state: tauri::State<'_, ServerControl>,
) -> Result<String, String> {
    let token = workbench_core::token::generate().map_err(|e| e.to_string())?;
    let mut settings =
        workbench_core::config::load_workbench_settings().map_err(|e| e.to_string())?;
    settings.server_token = Some(token.clone());
    workbench_core::config::save_workbench_settings(&settings).map_err(|e| e.to_string())?;
    state.restart_lan_with_token(token.clone()).await?;
    if let Err(e) = app.emit("settings:changed", ()) {
        log::warn!("failed to emit settings:changed after token rotation: {e}");
    }
    Ok(token)
}

/// This machine's IPv4 addresses for the pairing QR code, Tailscale first.
#[tauri::command]
pub fn pairing_addresses() -> Result<Vec<crate::net::PairingAddress>, String> {
    crate::net::pairing_addresses().map_err(|e| e.to_string())
}

/// Start the LAN server (opt-in server mode). Has no effect on the loopback
/// server. A different token than the running server's restarts it.
#[tauri::command]
pub async fn start_server(
    state: tauri::State<'_, ServerControl>,
    bind: Option<String>,
    port: u16,
    token: Option<String>,
) -> Result<ServerStatus, String> {
    let token = lan_token(token).ok_or_else(|| LAN_NEEDS_TOKEN.to_string())?;
    let bind = bind.unwrap_or_else(|| "0.0.0.0".to_string());
    let address = state.ensure_lan(&bind, port, token).await?;
    Ok(ServerStatus {
        running: true,
        address: Some(address),
        token: None,
    })
}

/// Stop the LAN server, disconnecting its clients. Has no effect on the
/// loopback server or on terminals.
#[tauri::command]
pub async fn stop_server(state: tauri::State<'_, ServerControl>) -> Result<ServerStatus, String> {
    let mut slot = state.lan.lock().await;
    if let Some(lan) = slot.take() {
        lan.handle.stop().await;
    }
    Ok(ServerStatus::stopped())
}

/// Status of the LAN server (used by the server-mode settings UI).
#[tauri::command]
pub async fn server_status(state: tauri::State<'_, ServerControl>) -> Result<ServerStatus, String> {
    Ok(match state.lan.lock().await.as_ref() {
        Some(lan) => ServerStatus {
            running: true,
            address: Some(lan.handle.addr().to_string()),
            token: None,
        },
        None => ServerStatus::stopped(),
    })
}

/// Status of the always-on loopback server. Used by the frontend's terminal
/// layer (`terminal-connection.ts`) to learn the `ws://127.0.0.1:<port>` base
/// URL and the token it must present.
#[tauri::command]
pub fn terminal_server_status(state: tauri::State<'_, ServerControl>) -> ServerStatus {
    let guard = state.loopback.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(lb) => ServerStatus {
            running: true,
            address: Some(lb.handle.addr().to_string()),
            token: Some(lb.token.clone()),
        },
        None => ServerStatus::stopped(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::Manager;

    const LAN_TOKEN: &str = "lan-token-0123456789abcdef0123456789";

    /// Build a headless mock Tauri app (no webview) holding the ServerControl state.
    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        mock_builder()
            .manage(ServerControl::new())
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    async fn start_lan(app: &tauri::App<tauri::test::MockRuntime>) -> ServerStatus {
        // Bind 127.0.0.1 so the test never exposes a publicly-routable port.
        start_server(
            app.state(),
            Some("127.0.0.1".to_string()),
            0,
            Some(LAN_TOKEN.to_string()),
        )
        .await
        .expect("start_server")
    }

    #[tokio::test]
    async fn lan_start_status_stop_cycle() {
        let app = mock_app();

        assert!(!server_status(app.state()).await.unwrap().running);
        assert!(!terminal_server_status(app.state()).running);

        let started = start_lan(&app).await;
        assert!(started.running);
        assert!(started.address.is_some());
        assert!(started.token.is_none(), "LAN status never echoes the token");

        let status = server_status(app.state()).await.unwrap();
        assert!(status.running);
        assert_eq!(status.address, started.address);

        // Starting again while running is a no-op that returns the same address
        // (the same token never rebinds).
        let again = start_server(app.state(), None, 0, Some(LAN_TOKEN.to_string()))
            .await
            .expect("second start is idempotent");
        assert_eq!(again.address, started.address);

        let stopped = stop_server(app.state()).await.expect("stop_server");
        assert!(!stopped.running);
        assert!(!server_status(app.state()).await.unwrap().running);

        // Stopping the LAN server must NOT affect the loopback slot.
        assert!(!terminal_server_status(app.state()).running);
    }

    #[tokio::test]
    async fn loopback_boot_exposes_a_strong_in_memory_token() {
        let app = mock_app();
        let sc: tauri::State<'_, ServerControl> = app.state();
        sc.start_loopback().await.expect("start loopback");

        let ts = terminal_server_status(app.state());
        assert!(ts.running);
        assert!(ts.address.as_deref().unwrap().starts_with("127.0.0.1:"));
        assert!(workbench_core::token::is_strong(
            ts.token.as_deref().unwrap()
        ));

        assert!(!server_status(app.state()).await.unwrap().running);
    }

    #[tokio::test]
    async fn loopback_and_lan_lifecycles_are_independent() {
        let app = mock_app();
        let sc: tauri::State<'_, ServerControl> = app.state();
        sc.start_loopback().await.expect("start loopback");

        let lan_addr = start_lan(&app).await.address.unwrap();

        let lb = terminal_server_status(app.state());
        assert!(lb.running);
        assert_ne!(lb.address.as_deref(), Some(lan_addr.as_str()));

        assert!(!stop_server(app.state()).await.expect("stop LAN").running);
        assert!(terminal_server_status(app.state()).running);
        assert!(!server_status(app.state()).await.unwrap().running);
    }

    /// A terminal in the shared managers is listed by both listeners, each
    /// behind its own token, and outlives a LAN restart.
    #[cfg(unix)]
    #[tokio::test]
    async fn listeners_share_terminals_across_lan_restarts() {
        let app = mock_app();
        let sc: tauri::State<'_, ServerControl> = app.state();
        sc.start_loopback().await.expect("start loopback");
        let tmp = tempfile::tempdir().unwrap();
        let meta = sc
            .managers
            .terminals
            .create(
                tmp.path().to_string_lossy().into_owned(),
                Some("shared".to_string()),
                None,
                80,
                24,
                None,
                None,
                None,
            )
            .expect("create terminal");

        let http = reqwest::Client::new();
        let list = |addr: String, token: String| {
            let http = http.clone();
            async move {
                let res = http
                    .get(format!("http://{addr}/remote/terminals"))
                    .bearer_auth(token)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(res.status(), 200);
                res.json::<serde_json::Value>().await.unwrap()
            }
        };
        let has_terminal = |v: &serde_json::Value| {
            v.as_array()
                .unwrap()
                .iter()
                .any(|t| t["id"] == meta.id.as_str())
        };

        let lb = terminal_server_status(app.state());
        let lb_listed = list(lb.address.clone().unwrap(), lb.token.clone().unwrap()).await;
        assert!(has_terminal(&lb_listed));

        let lan_addr = start_lan(&app).await.address.unwrap();
        assert!(has_terminal(&list(lan_addr, LAN_TOKEN.to_string()).await));

        stop_server(app.state()).await.expect("stop LAN");
        let lan_addr = start_lan(&app).await.address.unwrap();
        assert!(
            has_terminal(&list(lan_addr, LAN_TOKEN.to_string()).await),
            "terminals survive a LAN stop/start"
        );

        sc.managers.terminals.kill(&meta.id);
        stop_server(app.state()).await.expect("stop LAN");
    }

    #[tokio::test]
    async fn lan_server_refuses_a_missing_or_weak_token() {
        let app = mock_app();
        for token in [None, Some(""), Some("   "), Some("secret")] {
            let res = start_server(
                app.state(),
                Some("127.0.0.1".to_string()),
                0,
                token.map(str::to_string),
            )
            .await;
            assert_eq!(res.err().as_deref(), Some(LAN_NEEDS_TOKEN), "{token:?}");
        }
        assert!(!server_status(app.state()).await.unwrap().running);
    }

    #[test]
    fn generated_tokens_pass_the_lan_gate() {
        let token = workbench_core::token::generate().unwrap();
        assert_eq!(lan_token(Some(token.clone())), Some(token));
    }

    async fn lan_status_with(addr: &str, token: &str) -> u16 {
        reqwest::Client::new()
            .get(format!("http://{addr}/remote/terminals"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    }

    /// Rotating while running swaps the listener's token on the same port, so the
    /// old token stops working instead of lingering on an "already running" server.
    #[tokio::test]
    async fn rotating_the_token_restarts_a_running_lan_listener() {
        const ROTATED: &str = "rotated-token-abcdefabcdefabcdefabcdef";
        let app = mock_app();
        let sc: tauri::State<'_, ServerControl> = app.state();
        let addr = start_lan(&app).await.address.unwrap();
        assert_eq!(lan_status_with(&addr, LAN_TOKEN).await, 200);

        sc.restart_lan_with_token(ROTATED.to_string())
            .await
            .unwrap();

        let status = server_status(app.state()).await.unwrap();
        assert_eq!(
            status.address.as_deref(),
            Some(addr.as_str()),
            "same bind/port"
        );
        assert_eq!(sc.lan.lock().await.as_ref().unwrap().token, ROTATED);
        assert_eq!(lan_status_with(&addr, LAN_TOKEN).await, 401);
        assert_eq!(lan_status_with(&addr, ROTATED).await, 200);

        stop_server(app.state()).await.unwrap();
    }

    #[tokio::test]
    async fn starting_with_a_different_token_replaces_the_running_one() {
        const OTHER: &str = "other-token-abcdefabcdefabcdefabcdefab";
        let app = mock_app();
        let addr = start_lan(&app).await.address.unwrap();

        start_server(
            app.state(),
            Some("127.0.0.1".to_string()),
            0,
            Some(OTHER.to_string()),
        )
        .await
        .unwrap();

        let sc: tauri::State<'_, ServerControl> = app.state();
        let new_addr = server_status(app.state()).await.unwrap().address.unwrap();
        assert_eq!(sc.lan.lock().await.as_ref().unwrap().token, OTHER);
        assert_eq!(lan_status_with(&new_addr, OTHER).await, 200);
        assert_eq!(lan_status_with(&new_addr, LAN_TOKEN).await, 401);
        if new_addr != addr {
            assert!(
                reqwest::get(format!("http://{addr}/health")).await.is_err(),
                "old listener stopped"
            );
        }

        stop_server(app.state()).await.unwrap();
    }

    #[tokio::test]
    async fn rotating_while_stopped_leaves_server_mode_off() {
        let app = mock_app();
        let sc: tauri::State<'_, ServerControl> = app.state();
        sc.restart_lan_with_token(LAN_TOKEN.to_string())
            .await
            .unwrap();
        assert!(!server_status(app.state()).await.unwrap().running);
    }
}
