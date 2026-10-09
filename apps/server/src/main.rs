use clap::Parser;
use workbench_server::cli::Cli;

fn main() -> anyhow::Result<()> {
    // Before the runtime's threads start: changing the env isn't thread-safe.
    workbench_core::shell::scrub_inherited_env();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "workbench_server=info,tower_http=info".into()),
        )
        .init();
    workbench_server::watchdog::raise_fd_limit();
    workbench_server::watchdog::start(tokio::runtime::Handle::current());

    let token = cli.resolved_token()?;
    if token.is_some() {
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

    workbench_server::serve(&cli.bind, cli.port, token, shutdown_signal()).await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
