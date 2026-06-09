use crate::spawn::RemoteControlManager;
use crate::terminal::TerminalManager;

#[derive(Clone)]
pub struct AppState {
    pub spawn: RemoteControlManager,
    pub terminals: TerminalManager,
    /// When `Some`, requests must present this as a bearer token.
    pub token: Option<String>,
}
