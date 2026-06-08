use clap::Parser;

/// Headless Workbench control-plane server.
///
/// Exposes the same project / worktree / session operations the desktop app uses
/// over HTTP, plus an endpoint to spawn `claude remote-control` sessions on this
/// machine. Spawned sessions register with Anthropic's API and appear in the
/// Claude mobile app / claude.ai automatically — this server never proxies
/// terminal IO.
#[derive(Debug, Clone, Parser)]
#[command(name = "workbench-server", version, about)]
pub struct Cli {
    /// Address to bind. Defaults to all interfaces; secure it with a private
    /// network (e.g. Tailscale) rather than exposing it to the public internet.
    #[arg(long, env = "WORKBENCH_BIND", default_value = "0.0.0.0")]
    pub bind: String,

    /// TCP port to listen on.
    #[arg(long, env = "WORKBENCH_PORT", default_value_t = 4317)]
    pub port: u16,

    /// Optional bearer token. When set, every request (except `/health`) must
    /// send `Authorization: Bearer <token>`. Off by default — defense in depth
    /// on top of network-level security, not a replacement for it.
    #[arg(long, env = "WORKBENCH_TOKEN")]
    pub token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_bind_all_interfaces_on_4317_no_token() {
        let cli = Cli::parse_from(["workbench-server"]);
        assert_eq!(cli.bind, "0.0.0.0");
        assert_eq!(cli.port, 4317);
        assert!(cli.token.is_none());
    }

    #[test]
    fn explicit_flags_override_defaults() {
        let cli = Cli::parse_from([
            "workbench-server",
            "--bind",
            "127.0.0.1",
            "--port",
            "9000",
            "--token",
            "secret",
        ]);
        assert_eq!(cli.bind, "127.0.0.1");
        assert_eq!(cli.port, 9000);
        assert_eq!(cli.token.as_deref(), Some("secret"));
    }

    #[test]
    fn rejects_a_non_numeric_port() {
        assert!(Cli::try_parse_from(["workbench-server", "--port", "not-a-port"]).is_err());
    }
}
