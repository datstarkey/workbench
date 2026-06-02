use clap::Parser;
use workbench_server::cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "workbench_server=info,tower_http=info".into()),
        )
        .init();

    if cli.token.is_some() {
        tracing::info!(
            "workbench-server listening on {}:{} (bearer token required)",
            cli.bind,
            cli.port
        );
    } else {
        tracing::warn!(
            "workbench-server listening on {}:{} with NO auth — secure it with a private network (e.g. Tailscale)",
            cli.bind,
            cli.port
        );
    }

    workbench_server::serve(&cli.bind, cli.port, cli.token, shutdown_signal()).await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
