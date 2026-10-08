use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use tokio::sync::watch;

use crate::agent::AgentManager;
use crate::host::HostControl;
use crate::terminal::TerminalManager;
use crate::usage::{ModelsCache, UsageCache};

/// The long-lived session managers. Both are `Arc`-backed, so clones share the
/// same terminals and chats — which is how the desktop's loopback and
/// LAN listeners expose one set of terminals.
#[derive(Clone, Default)]
pub struct Managers {
    pub terminals: TerminalManager,
    pub agents: AgentManager,
    pub usage: UsageCache,
    pub models: ModelsCache,
    /// The app embedding this server, when it can update itself (the desktop).
    pub host: Option<Arc<dyn HostControl>>,
}

impl Managers {
    /// Blocking: kills every terminal and chat process and waits for them.
    pub fn kill_all(&self) {
        self.terminals.kill_all();
        self.agents.kill_all();
    }
}

#[derive(Clone)]
pub struct AppState {
    pub terminals: TerminalManager,
    pub agents: AgentManager,
    pub usage: UsageCache,
    pub models: ModelsCache,
    pub host: Option<Arc<dyn HostControl>>,
    /// When `Some`, requests must present this as a bearer token. Only the
    /// standalone binary on a loopback bind (or with `--insecure-no-token`) runs
    /// with `None`; embedded listeners always carry one.
    pub token: Option<String>,
    /// Flips to `true` when this listener stops. Upgraded WebSockets outlive the
    /// listener's graceful shutdown (they run in detached tasks), so each attach
    /// watches this and disconnects — otherwise a revoked token keeps typing.
    pub revoked: watch::Receiver<bool>,
    /// The port this listener is bound to, so a terminal's plugin can reach
    /// it on loopback (`mod_routes`). `None` in tests that build state by hand.
    pub local_port: Option<u16>,
}

impl AppState {
    pub fn new(managers: Managers, token: Option<String>, revoked: watch::Receiver<bool>) -> Self {
        Self {
            terminals: managers.terminals,
            agents: managers.agents,
            usage: managers.usage,
            models: managers.models,
            host: managers.host,
            token,
            revoked,
            local_port: None,
        }
    }

    pub fn with_local_port(mut self, port: u16) -> Self {
        self.local_port = Some(port);
        // Terminal plugins reach the first listener (the desktop's loopback one),
        // which outlives a LAN listener that server mode turns off.
        self.agents.bind_terminals(self.terminals.clone(), port);
        self
    }
}

/// Resolve once `revoked` is set. A dropped sender never resolves: dropping a
/// [`crate::ServerHandle`] deliberately leaves the server running.
pub(crate) async fn wait_revoked(revoked: &mut watch::Receiver<bool>) {
    loop {
        if *revoked.borrow_and_update() {
            return;
        }
        if revoked.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// How long a WebSocket send may wait on a client that stopped reading (a
/// backgrounded phone keeps its TCP connection open) before the socket is dropped.
const WS_SEND_TIMEOUT: Duration = Duration::from_secs(30);
/// Plus a second per this many bytes, so a big frame (a long chat's snapshot, a
/// scrollback replay) on a slow but live link isn't cut off.
const WS_SLOW_LINK_BYTES_PER_SEC: usize = 16 * 1024;

/// Send one frame, giving up after [`ws_send_timeout`]. `false` means the socket
/// is dead or stalled: the caller drops it rather than retry, so a pending send
/// can't pin its task and keep a revoke or takeover from being delivered.
pub(crate) async fn ws_send(socket: &mut WebSocket, msg: Message) -> bool {
    let limit = ws_send_timeout(&msg);
    matches!(
        tokio::time::timeout(limit, socket.send(msg)).await,
        Ok(Ok(()))
    )
}

fn ws_send_timeout(msg: &Message) -> Duration {
    let len = match msg {
        Message::Text(text) => text.len(),
        Message::Binary(bytes) => bytes.len(),
        _ => 0,
    };
    WS_SEND_TIMEOUT + Duration::from_secs((len / WS_SLOW_LINK_BYTES_PER_SEC) as u64)
}

/// Send a last frame (if any), then a close frame, both under the send timeout.
pub(crate) async fn ws_close(mut socket: WebSocket, last: Option<Message>) {
    if let Some(msg) = last {
        if !ws_send(&mut socket, msg).await {
            return;
        }
    }
    let _ = tokio::time::timeout(WS_SEND_TIMEOUT, socket.close()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_big_frame_gets_longer_to_send() {
        assert_eq!(ws_send_timeout(&Message::Close(None)), WS_SEND_TIMEOUT);
        let snapshot = Message::Text("x".repeat(4 * 1024 * 1024));
        assert_eq!(
            ws_send_timeout(&snapshot),
            WS_SEND_TIMEOUT + Duration::from_secs(256)
        );
    }
}
