use tokio::sync::watch;

use crate::spawn::RemoteControlManager;
use crate::terminal::TerminalManager;

/// The long-lived session managers. Both are `Arc`-backed, so clones share the
/// same terminals and spawned sessions — which is how the desktop's loopback and
/// LAN listeners expose one set of terminals.
#[derive(Clone, Default)]
pub struct Managers {
    pub spawn: RemoteControlManager,
    pub terminals: TerminalManager,
}

impl Managers {
    /// Blocking: kills every terminal and spawned session and waits for them.
    pub fn kill_all(&self) {
        self.terminals.kill_all();
        self.spawn.kill_all();
    }
}

#[derive(Clone)]
pub struct AppState {
    pub spawn: RemoteControlManager,
    pub terminals: TerminalManager,
    /// When `Some`, requests must present this as a bearer token. Only the
    /// standalone binary on a loopback bind (or with `--insecure-no-token`) runs
    /// with `None`; embedded listeners always carry one.
    pub token: Option<String>,
    /// Flips to `true` when this listener stops. Upgraded WebSockets outlive the
    /// listener's graceful shutdown (they run in detached tasks), so each attach
    /// watches this and disconnects — otherwise a revoked token keeps typing.
    pub revoked: watch::Receiver<bool>,
}

impl AppState {
    pub fn new(managers: Managers, token: Option<String>, revoked: watch::Receiver<bool>) -> Self {
        Self {
            spawn: managers.spawn,
            terminals: managers.terminals,
            token,
            revoked,
        }
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
