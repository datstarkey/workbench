//! Workbench control-plane server, usable both as a standalone binary and
//! embedded inside the desktop app ("server mode").

pub mod auth;
pub mod cli;
pub mod error;
pub mod routes;
pub mod spawn;
pub mod state;
pub mod terminal;

use anyhow::Context;
use std::net::SocketAddr;
use tokio::sync::{oneshot, watch};

pub use spawn::RemoteControlManager;
pub use state::{AppState, Managers};
pub use terminal::TerminalManager;

/// Build the full router (control-plane routes + bearer auth + CORS).
pub fn app(state: AppState) -> axum::Router {
    use axum::http::{header, Method};
    use tower_http::cors::{AllowOrigin, CorsLayer};

    // Any origin is safe only because auth is a bearer header, never a cookie:
    // a foreign page can't attach credentials it doesn't know. So credentials
    // mode stays off — never add `allow_credentials` without revisiting this.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::any())
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    routes::router(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::require_bearer,
        ))
        .layer(cors)
}

/// Serve until `shutdown` resolves (or forever if it never does). Returns the
/// bound address via `on_bound` so embedders can learn the actual port when
/// binding port 0.
pub async fn serve(
    bind: &str,
    port: u16,
    token: Option<String>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let (revoke, revoked) = watch::channel(false);
    let app = app(AppState::new(Managers::default(), token, revoked));
    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.await;
            let _ = revoke.send(true);
        })
        .await
        .context("server error")?;
    Ok(())
}

/// Handle for a server embedded in another process (e.g. the desktop app).
/// Dropping it does not stop the server; call [`ServerHandle::stop`].
pub struct ServerHandle {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    revoke: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl ServerHandle {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Disconnect this listener's attached terminal WebSockets, then shut it
    /// down gracefully and wait for the server task. Terminals themselves live
    /// in the shared managers and stay attachable through other listeners.
    pub async fn stop(mut self) {
        let _ = self.revoke.send(true);
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

/// Spawn the server on the current Tokio runtime and return a handle. Binds
/// before returning so the caller knows the server is listening (and on which
/// port, useful when `port` is 0).
///
/// `managers` are shared with any other listener built from clones of them.
/// The token is mandatory and must pass [`workbench_core::token::is_strong`]:
/// an embedded listener is never unauthenticated.
pub async fn spawn_embedded(
    bind: &str,
    port: u16,
    managers: Managers,
    token: String,
) -> anyhow::Result<ServerHandle> {
    anyhow::ensure!(
        workbench_core::token::is_strong(&token),
        "embedded server requires a token of at least {} characters",
        workbench_core::token::MIN_TOKEN_LEN
    );
    let (revoke, revoked) = watch::channel(false);
    let app = app(AppState::new(managers, Some(token), revoked));
    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    let local_addr = listener.local_addr().context("failed to read local addr")?;

    let (tx, rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });

    Ok(ServerHandle {
        addr: local_addr,
        shutdown: Some(tx),
        revoke,
        task,
    })
}
