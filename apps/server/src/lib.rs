//! Workbench control-plane server, usable both as a standalone binary and
//! embedded inside the desktop app ("server mode").

pub mod auth;
pub mod cli;
pub mod error;
pub mod routes;
pub mod spawn;
pub mod state;

use anyhow::Context;
use std::net::SocketAddr;
use tokio::sync::oneshot;

pub use spawn::RemoteControlManager;
pub use state::AppState;

/// Build the full router (control-plane routes + optional bearer auth + CORS).
pub fn app(state: AppState) -> axum::Router {
    routes::router(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::require_bearer,
        ))
        // Browser/mobile clients fetch cross-origin; allow it (the server is
        // already network-secured, not origin-secured).
        .layer(tower_http::cors::CorsLayer::permissive())
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
    let state = AppState {
        spawn: RemoteControlManager::new(),
        token,
    };
    let app = app(state);
    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .context("server error")?;
    Ok(())
}

/// Handle for a server embedded in another process (e.g. the desktop app).
/// Dropping it does not stop the server; call [`ServerHandle::stop`].
pub struct ServerHandle {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl ServerHandle {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Signal graceful shutdown and wait for the server task to finish.
    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

/// Spawn the server on the current Tokio runtime and return a handle. Binds
/// before returning so the caller knows the server is listening (and on which
/// port, useful when `port` is 0).
pub async fn spawn_embedded(
    bind: &str,
    port: u16,
    token: Option<String>,
) -> anyhow::Result<ServerHandle> {
    let state = AppState {
        spawn: RemoteControlManager::new(),
        token,
    };
    let app = app(state);
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
        task,
    })
}
