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

#[derive(Clone)]
pub struct AppState {
    pub spawn: RemoteControlManager,
    pub terminals: TerminalManager,
    /// When `Some`, requests must present this as a bearer token. Only the
    /// standalone binary on a loopback bind (or with `--insecure-no-token`) runs
    /// with `None`; embedded listeners always carry one.
    pub token: Option<String>,
}

impl AppState {
    pub fn new(managers: Managers, token: Option<String>) -> Self {
        Self {
            spawn: managers.spawn,
            terminals: managers.terminals,
            token,
        }
    }
}
