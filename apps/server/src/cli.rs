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

    /// Bearer token (at least 32 characters, e.g. `openssl rand -hex 32`). Every
    /// request except `/health` must send `Authorization: Bearer <token>`.
    /// Required unless binding a loopback address.
    #[arg(long, env = "WORKBENCH_TOKEN")]
    pub token: Option<String>,

    /// Run without a token on a non-loopback bind. Anyone who can reach the port
    /// gets a shell on this machine.
    #[arg(long)]
    pub insecure_no_token: bool,
}

impl Cli {
    /// The token to serve with, or an error when the configuration would expose
    /// an unauthenticated (or weakly authenticated) server to the network.
    pub fn resolved_token(&self) -> anyhow::Result<Option<String>> {
        match self.token.as_deref().filter(|t| !t.trim().is_empty()) {
            Some(token) if workbench_core::token::is_strong(token) => Ok(Some(token.to_string())),
            Some(_) => anyhow::bail!(
                "--token must be at least {} characters with no whitespace (try `openssl rand -hex 32`)",
                workbench_core::token::MIN_TOKEN_LEN
            ),
            None if self.insecure_no_token || is_loopback(&self.bind) => Ok(None),
            None => anyhow::bail!(
                "refusing to listen on {} without a token: pass --token (or WORKBENCH_TOKEN), \
                 bind a loopback address, or pass --insecure-no-token",
                self.bind
            ),
        }
    }
}

fn is_loopback(bind: &str) -> bool {
    bind.eq_ignore_ascii_case("localhost")
        || bind
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRONG: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn defaults_bind_all_interfaces_on_4317_no_token() {
        let cli = Cli::parse_from(["workbench-server"]);
        assert_eq!(cli.bind, "0.0.0.0");
        assert_eq!(cli.port, 4317);
        assert!(cli.token.is_none());
        assert!(!cli.insecure_no_token);
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

    #[test]
    fn a_network_bind_without_a_token_is_refused() {
        let cli = Cli::parse_from(["workbench-server"]);
        assert!(cli.resolved_token().is_err());
        let blank = Cli::parse_from(["workbench-server", "--token", "  "]);
        assert!(blank.resolved_token().is_err());
    }

    #[test]
    fn loopback_or_explicit_opt_out_may_run_without_a_token() {
        for bind in ["127.0.0.1", "::1", "[::1]", "localhost"] {
            let cli = Cli::parse_from(["workbench-server", "--bind", bind]);
            assert_eq!(cli.resolved_token().unwrap(), None, "{bind}");
        }
        let cli = Cli::parse_from(["workbench-server", "--insecure-no-token"]);
        assert_eq!(cli.resolved_token().unwrap(), None);
    }

    #[test]
    fn short_tokens_are_refused_even_on_loopback() {
        let cli = Cli::parse_from([
            "workbench-server",
            "--bind",
            "127.0.0.1",
            "--token",
            "secret",
        ]);
        assert!(cli.resolved_token().is_err());
    }

    #[test]
    fn a_strong_token_is_used() {
        let cli = Cli::parse_from(["workbench-server", "--token", STRONG]);
        assert_eq!(cli.resolved_token().unwrap().as_deref(), Some(STRONG));
    }
}
