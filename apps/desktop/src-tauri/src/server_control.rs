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
use workbench_server::{Managers, ServerHandle};

struct Loopback {
    handle: ServerHandle,
    token: String,
}

/// Managed Tauri state holding the shared managers and both listener handles.
#[derive(Default)]
pub struct ServerControl {
    managers: Managers,
    loopback: Mutex<Option<Loopback>>,
    lan: Mutex<Option<ServerHandle>>,
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

/// Ensure the LAN listener is running. Returns the current address on success
/// (whether a new server was started or one was already running). If a
/// concurrent call wins the race, the freshly-spawned server is stopped to
/// avoid orphans.
async fn ensure_lan_started(
    state: &ServerControl,
    bind: &str,
    port: u16,
    token: String,
) -> Result<String, String> {
    {
        let guard = state.lan.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = guard.as_ref() {
            return Ok(handle.addr().to_string());
        }
    }

    let handle = workbench_server::spawn_embedded(bind, port, state.managers.clone(), token)
        .await
        .map_err(|e| e.to_string())?;
    let address = handle.addr().to_string();

    // Re-check after the await: a concurrent call may have won the race.
    let existing_addr = {
        let guard = state.lan.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(|h| h.addr().to_string())
    };
    if let Some(addr) = existing_addr {
        handle.stop().await;
        return Ok(addr);
    }

    let mut guard = state.lan.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(handle);

    Ok(address)
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

/// Generate a new LAN server token (the settings UI saves it).
#[tauri::command]
pub fn generate_server_token() -> Result<String, String> {
    workbench_core::token::generate().map_err(|e| e.to_string())
}

/// Start the LAN server (opt-in server mode). Has no effect on the loopback
/// server.
#[tauri::command]
pub async fn start_server(
    state: tauri::State<'_, ServerControl>,
    bind: Option<String>,
    port: u16,
    token: Option<String>,
) -> Result<ServerStatus, String> {
    let token = lan_token(token).ok_or_else(|| LAN_NEEDS_TOKEN.to_string())?;
    let bind = bind.unwrap_or_else(|| "0.0.0.0".to_string());
    let address = ensure_lan_started(&state, &bind, port, token).await?;
    Ok(ServerStatus {
        running: true,
        address: Some(address),
        token: None,
    })
}

/// Stop the LAN server. Has no effect on the loopback server or on terminals.
#[tauri::command]
pub async fn stop_server(state: tauri::State<'_, ServerControl>) -> Result<ServerStatus, String> {
    let handle = {
        let mut guard = state.lan.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    if let Some(handle) = handle {
        handle.stop().await;
    }
    Ok(ServerStatus::stopped())
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
        None => ServerStatus::stopped(),
    }
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

        assert!(!server_status(app.state()).running);
        assert!(!terminal_server_status(app.state()).running);

        let started = start_lan(&app).await;
        assert!(started.running);
        assert!(started.address.is_some());
        assert!(started.token.is_none(), "LAN status never echoes the token");

        let status = server_status(app.state());
        assert!(status.running);
        assert_eq!(status.address, started.address);

        // Starting again while running is a no-op that returns the same address
        // (exercises the double-checked-lock guard, not a second bind).
        let again = start_server(app.state(), None, 0, Some(LAN_TOKEN.to_string()))
            .await
            .expect("second start is idempotent");
        assert_eq!(again.address, started.address);

        let stopped = stop_server(app.state()).await.expect("stop_server");
        assert!(!stopped.running);
        assert!(!server_status(app.state()).running);

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

        assert!(!server_status(app.state()).running);
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
        assert!(!server_status(app.state()).running);
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
        assert!(!server_status(app.state()).running);
    }

    #[test]
    fn generated_server_tokens_pass_the_lan_gate() {
        let token = generate_server_token().unwrap();
        assert_eq!(lan_token(Some(token.clone())), Some(token));
    }
}
