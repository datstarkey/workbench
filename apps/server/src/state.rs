use crate::spawn::RemoteControlManager;

#[derive(Clone)]
pub struct AppState {
    pub spawn: RemoteControlManager,
    /// When `Some`, requests must present this as a bearer token.
    pub token: Option<String>,
}
