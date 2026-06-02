mod auth;
mod cli;
mod error;
mod routes;
mod spawn;
mod state;

use anyhow::Context;
use clap::Parser;
use tower_http::trace::TraceLayer;

use crate::cli::Cli;
use crate::spawn::RemoteControlManager;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "workbench_server=info,tower_http=info".into()),
        )
        .init();

    let state = AppState {
        spawn: RemoteControlManager::new(),
        token: cli.token.clone(),
    };

    let app = routes::router(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            auth::require_bearer,
        ))
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", cli.bind, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    if cli.token.is_some() {
        tracing::info!("workbench-server listening on {addr} (bearer token required)");
    } else {
        tracing::warn!(
            "workbench-server listening on {addr} with NO auth — secure it with a private network (e.g. Tailscale)"
        );
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
