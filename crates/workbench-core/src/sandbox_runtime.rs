//! Generates the settings file for `@anthropic-ai/sandbox-runtime` (`srt`), the
//! wrapper Workbench puts in front of `claude` when the sandbox-runtime setting
//! is on.
//!
//! `srt` confines the whole `claude` process — file tools, MCP servers and hooks,
//! not just Bash — behind Seatbelt (macOS) or bubblewrap (Linux). The generated
//! file keeps project folders writable, leaves the rest of the filesystem
//! read-only, hides credential stores, and allows only the domains Claude Code
//! needs plus whatever the user added.
//!
//! Four things this module must get right, all verified against srt 0.0.76:
//!
//! * **`~/.claude` is an allowlist, not a denylist.** That directory accumulates
//!   new code-execution surfaces (hooks, plugins, skills, agents, commands,
//!   workflows, routines, daemon state, …) faster than a denylist can track, so
//!   only the runtime-state paths Claude Code actually writes are allowed and
//!   everything else is unwritable by default.
//! * **srt's mandatory write denies only cover the process cwd.** A second
//!   project root listed in `allowWrite` has a fully writable `.git/hooks`,
//!   `.git/config` and `.claude/`, so those are emitted per project root.
//! * **srt matches on the real path.** `/tmp` in `allowWrite` grants nothing on
//!   macOS, because `/tmp` is a symlink to `/private/tmp`; the same applies to
//!   `/var` (hence `$TMPDIR`) and to any project under a symlinked directory.
//!   Every path is therefore canonicalized before it is emitted.
//! * **Every path is emitted absolute.** srt does expand `~`, but relying on it
//!   would make the file's meaning depend on srt's own home resolution.
//!
//! `srt` rejects a settings file that is missing any required key, so every
//! field below is always emitted (empty arrays included). A rejected file fails
//! loudly rather than silently running unconfined — srt prints
//! `Invalid configuration in <path>` followed by the offending key, then
//! `Refusing to run with the default config`.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::paths;
use crate::types::{ProjectConfig, WorkbenchSettings};

/// Filename under the Workbench config dir.
pub const SETTINGS_FILE: &str = "sandbox-runtime.json";

/// Domains Claude Code itself needs. Mirrors the "Network access requirements"
/// table in https://code.claude.com/docs/en/network-config, trimmed to the hosts
/// a Workbench-launched CLI session actually reaches.
///
/// These are always emitted: the user's list is unioned with them, never
/// replaced, so removing an entry in the settings UI cannot break Claude.
pub const DEFAULT_ALLOWED_DOMAINS: &[&str] = &[
    "api.anthropic.com",
    "claude.ai",
    "claude.com",
    "platform.claude.com",
    "mcp-proxy.anthropic.com",
    "downloads.claude.ai",
    "statsig.anthropic.com",
    "*.frame.claudeusercontent.com",
    "code.claude.com",
];

/// Runtime-state paths under `~/.claude` that Claude Code must be able to write.
///
/// This is an allowlist because `~/.claude` is not a fixed shape: it gains new
/// code-execution surfaces over time (hooks, plugins, skills, agents, commands,
/// output styles, rules, workflows, routines, scheduled tasks, daemon state, the
/// CLI's own `local` install dir …), and a denylist silently loses that race.
///
/// Determined against Claude Code 2.1.270: with `~/.claude` entirely unwritable a
/// `-p` session still completes and Bash still works, but **no session JSONL is
/// written**, which silently breaks Workbench's session discovery, resume and
/// labels. `projects` is therefore load-bearing; the rest is Claude Code runtime
/// state allowed pre-emptively so a session is never wedged by a denied write.
///
/// Deliberately absent: `shell-snapshots`. A session with it denied ran Bash
/// successfully, so it is not required.
const CLAUDE_ALLOW_WRITE: &[&str] = &[
    // Session JSONL — Workbench reads this for discovery, resume and labels.
    "projects",
    "todos",
    "sessions",
    "history.jsonl",
    "file-history",
    "plans",
    "paste-cache",
    "statsig",
    "telemetry",
    "cache",
    "stats-cache.json",
    "ide",
    "debug",
    ".update.lock",
    // Rewritten on OAuth token refresh.
    ".credentials.json",
    "mcp-needs-auth-cache.json",
];

/// Paths under `~/.claude` explicitly denied for writing.
///
/// Redundant while [`CLAUDE_ALLOW_WRITE`] is the only thing opening that
/// directory up — writes are deny-by-default — but kept as defence in depth on
/// the worst offenders, so a future widening of the allowlist cannot
/// accidentally re-expose them.
const CLAUDE_DENY_WRITE: &[&str] = &[
    "settings.json",
    "settings.local.json",
    "hooks",
    "plugins",
    "local",
    "skills",
    "agents",
    "commands",
    "shell-snapshots",
];

/// Per-project-root paths denied for writing.
///
/// srt's own mandatory denies stop at the process cwd, so a second project root
/// in `allowWrite` would otherwise have all of these writable. `.claude/*` here
/// is the project-scoped twin of the `~/.claude` surfaces, and `.git/modules`
/// holds submodule hook directories.
const PROJECT_DENY_WRITE: &[&str] = &[
    ".git/hooks",
    ".git/config",
    ".git/modules",
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".claude/hooks",
    ".claude/agents",
    ".claude/skills",
    ".claude/commands",
    ".mcp.json",
];

/// Credential stores a sandboxed session must not read.
///
/// `~/.claude/.credentials.json` is deliberately absent: Claude needs its own
/// OAuth token to run at all.
const HOME_DENY_READ: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".netrc",
    ".config/gh",
    ".docker",
    ".kube",
];

pub fn default_allowed_domains() -> Vec<String> {
    DEFAULT_ALLOWED_DOMAINS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemConfig {
    pub allow_write: Vec<String>,
    pub deny_write: Vec<String>,
    pub deny_read: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfig {
    pub allowed_domains: Vec<String>,
    /// Required by srt's schema even when empty.
    pub denied_domains: Vec<String>,
    pub allow_local_binding: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SandboxRuntimeConfig {
    pub filesystem: FilesystemConfig,
    pub network: NetworkConfig,
}

/// Absolute path of the generated settings file.
pub fn settings_path() -> PathBuf {
    paths::workbench_config_dir().join(SETTINGS_FILE)
}

/// Build the config without touching the filesystem — the unit-testable half of
/// [`write_settings`].
///
/// `hook_socket` is the `host:port` of the Workbench hook bridge, which the
/// Claude hook script dials over loopback. It changes on every app launch, so
/// the file has to be regenerated after the bridge binds.
pub fn build_config(
    settings: &WorkbenchSettings,
    projects: &[ProjectConfig],
    hook_socket: Option<&str>,
    home: &Path,
    config_dir: &Path,
) -> SandboxRuntimeConfig {
    let claude_dir = home.join(".claude");

    // `.` is the pane's cwd; project roots are listed too because a sibling-layout
    // worktree writes its metadata into the *main* repo's `.git/worktrees/*`.
    //
    // The Workbench config dir is deliberately NOT writable: a session that could
    // rewrite `settings.json` or `projects.json` would widen its own sandbox at
    // the next regeneration. Nothing inside the sandbox needs it — the hook
    // script only opens a TCP connection.
    let mut allow_write = vec![".".to_string()];
    allow_write.extend(projects.iter().map(|p| resolved(Path::new(&p.path))));
    // Narrow allowlist rather than the whole of ~/.claude — see CLAUDE_ALLOW_WRITE.
    allow_write.extend(
        CLAUDE_ALLOW_WRITE
            .iter()
            .map(|p| resolved(&claude_dir.join(p))),
    );
    allow_write.push(resolved(Path::new("/tmp")));
    if let Some(tmpdir) = std::env::var_os("TMPDIR") {
        let tmpdir = tmpdir.to_string_lossy().to_string();
        if !tmpdir.is_empty() {
            allow_write.push(resolved(Path::new(&tmpdir)));
        }
    }
    dedupe(&mut allow_write);

    let mut deny_write = Vec::new();
    // `mcpServers` here is a list of commands the user's next *unsandboxed*
    // `claude` will execute, so it is read-only despite living outside ~/.claude.
    deny_write.push(resolved(&home.join(".claude.json")));
    deny_write.extend(
        CLAUDE_DENY_WRITE
            .iter()
            .map(|p| resolved(&claude_dir.join(p))),
    );
    // Redundant while the config dir is absent from allowWrite, but keeps the
    // sandbox sealed if a project root is ever an ancestor of it.
    deny_write.push(resolved(config_dir));
    for project in projects {
        let root = Path::new(&project.path);
        deny_write.extend(PROJECT_DENY_WRITE.iter().map(|p| resolved(&root.join(p))));
    }
    dedupe(&mut deny_write);

    let mut deny_read: Vec<String> = HOME_DENY_READ
        .iter()
        .map(|p| resolved(&home.join(p)))
        .collect();
    // Holds the Trello API token.
    deny_read.push(resolved(&config_dir.join("settings.json")));
    dedupe(&mut deny_read);

    let mut allowed_domains = default_allowed_domains();
    allowed_domains.extend(settings.sandbox_allowed_domains.iter().cloned());
    // srt accepts a bare `host:port` allowlist entry, IP literals included —
    // validated against srt 0.0.76. A future schema change would surface as
    // `Invalid configuration in <path>: - network.allowedDomains: …` and srt
    // refusing to start, not as a silently unconfined session.
    if let Some(socket) = hook_socket {
        allowed_domains.push(socket.to_string());
    }
    dedupe(&mut allowed_domains);

    SandboxRuntimeConfig {
        filesystem: FilesystemConfig {
            allow_write,
            deny_write,
            deny_read,
        },
        network: NetworkConfig {
            allowed_domains,
            denied_domains: Vec::new(),
            // Dev servers bind local ports; that is inbound-only and does not
            // widen egress. Note loopback is unfiltered, so server mode must
            // carry a token — enforced in `server_control::start_server`.
            allow_local_binding: true,
        },
    }
}

/// Write the settings file and return its path.
pub fn write_settings(
    settings: &WorkbenchSettings,
    projects: &[ProjectConfig],
    hook_socket: Option<&str>,
) -> Result<PathBuf> {
    write_settings_to(
        &paths::home_dir(),
        &paths::workbench_config_dir(),
        settings,
        projects,
        hook_socket,
    )
}

fn write_settings_to(
    home: &Path,
    config_dir: &Path,
    settings: &WorkbenchSettings,
    projects: &[ProjectConfig],
    hook_socket: Option<&str>,
) -> Result<PathBuf> {
    let config = build_config(settings, projects, hook_socket, home, config_dir);
    let path = config_dir.join(SETTINGS_FILE);
    paths::atomic_write(&path, &serde_json::to_string_pretty(&config)?)?;
    Ok(path)
}

/// Emit `path` as an absolute, symlink-resolved string.
///
/// srt matches on the real path, so an unresolved symlink grants nothing: `/tmp`
/// in `allowWrite` does not make `/tmp` writable on macOS, because the kernel
/// sees `/private/tmp`. The same trap applies to `/var` (and therefore
/// `$TMPDIR`) and to any project checked out under a symlinked parent.
///
/// Paths that do not exist yet still need resolving (a `denyWrite` entry for a
/// file the user has never created, for instance), so the longest existing
/// ancestor is canonicalized and the remainder re-appended.
fn resolved(path: &Path) -> String {
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path;
    loop {
        if let Ok(real) = cursor.canonicalize() {
            let mut out = real;
            out.extend(suffix.iter().rev());
            return out.to_string_lossy().to_string();
        }
        match (cursor.parent(), cursor.file_name()) {
            (Some(parent), Some(name)) => {
                suffix.push(name.to_os_string());
                cursor = parent;
            }
            // Nothing on this path exists (a fake home in tests, say) — emit it
            // as given rather than inventing a resolution.
            _ => return path.to_string_lossy().to_string(),
        }
    }
}

fn dedupe(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|v| !v.is_empty() && seen.insert(v.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    // Must not exist on the host: `resolved()` canonicalizes, and on macOS even
    // `/home` is a real autofs mount that resolves to /System/Volumes/Data/home.
    const HOME: &str = "/wb-test-home/u";
    const CONFIG_DIR: &str = "/wb-test-home/u/.workbench";

    fn settings(domains: &[&str]) -> WorkbenchSettings {
        WorkbenchSettings {
            sandbox_allowed_domains: domains.iter().map(|d| d.to_string()).collect(),
            ..Default::default()
        }
    }

    fn project(path: &str) -> ProjectConfig {
        ProjectConfig {
            name: "p".to_string(),
            path: path.to_string(),
            group: None,
            shell: None,
            startup_command: None,
            tasks: Vec::new(),
        }
    }

    fn build(
        domains: &[&str],
        projects: &[ProjectConfig],
        hook: Option<&str>,
    ) -> SandboxRuntimeConfig {
        build_config(
            &settings(domains),
            projects,
            hook,
            Path::new(HOME),
            Path::new(CONFIG_DIR),
        )
    }

    fn assert_contains(list: &[String], expected: &str, label: &str) {
        assert!(
            list.contains(&expected.to_string()),
            "{expected} missing from {label}: {list:?}"
        );
    }

    // --- allowWrite ---

    #[test]
    fn cwd_and_project_roots_are_writable() {
        let cfg = build(&[], &[project("/repos/a"), project("/repos/b")], None);
        assert_eq!(cfg.filesystem.allow_write[0], ".");
        assert_contains(&cfg.filesystem.allow_write, "/repos/a", "allowWrite");
        assert_contains(&cfg.filesystem.allow_write, "/repos/b", "allowWrite");
    }

    /// Workbench reads session JSONL out of `~/.claude/projects`; with it denied a
    /// session completes but writes nothing, silently breaking discovery/resume.
    #[test]
    fn claude_session_state_is_writable() {
        let cfg = build(&[], &[], None);
        for name in ["projects", "todos", "history.jsonl", ".credentials.json"] {
            assert_contains(
                &cfg.filesystem.allow_write,
                &format!("/wb-test-home/u/.claude/{name}"),
                "allowWrite",
            );
        }
    }

    /// The whole point of the allowlist: `~/.claude` gains new code-execution
    /// surfaces over time, so the directory itself must never be opened up.
    #[test]
    fn claude_dir_itself_is_never_writable() {
        let cfg = build(&[], &[], None);
        assert!(
            !cfg.filesystem
                .allow_write
                .iter()
                .any(|p| p == "/wb-test-home/u/.claude"),
            "~/.claude must not be writable wholesale: {:?}",
            cfg.filesystem.allow_write
        );
    }

    /// Anything not on the allowlist — including surfaces that did not exist when
    /// this was written — is unwritable without needing a denylist entry.
    #[test]
    fn unlisted_claude_surfaces_are_not_writable() {
        let cfg = build(&[], &[], None);
        for name in [
            "workflows",
            "routines",
            "scheduled_tasks.json",
            "launch.json",
            "cowork_plugins",
            "remote-settings.json",
            "daemon.json",
            "policy-limits.json",
            "state",
            "jobs",
            "daemon",
            "output-styles",
            "rules",
            "backups",
            "shell-snapshots",
        ] {
            let path = format!("/wb-test-home/u/.claude/{name}");
            assert!(
                !cfg.filesystem.allow_write.contains(&path),
                "{path} must not be writable"
            );
        }
    }

    #[test]
    fn tmp_is_writable() {
        let cfg = build(&[], &[], None);
        // Resolved, because `/tmp` itself grants nothing on macOS.
        assert_contains(
            &cfg.filesystem.allow_write,
            &resolved(Path::new("/tmp")),
            "allowWrite",
        );
    }

    /// A session that could rewrite `settings.json` or `projects.json` would
    /// widen its own sandbox at the next regeneration.
    #[test]
    fn workbench_config_dir_is_never_writable() {
        let cfg = build(&[], &[], None);
        assert!(
            !cfg.filesystem.allow_write.iter().any(|p| p == CONFIG_DIR),
            "config dir must not be writable: {:?}",
            cfg.filesystem.allow_write
        );
        assert_contains(&cfg.filesystem.deny_write, CONFIG_DIR, "denyWrite");
    }

    /// `~/.claude.json` carries `mcpServers`, a list of commands the next
    /// unsandboxed `claude` executes.
    #[test]
    fn claude_json_is_not_writable() {
        let cfg = build(&[], &[], None);
        assert!(
            !cfg.filesystem
                .allow_write
                .iter()
                .any(|p| p == "/wb-test-home/u/.claude.json"),
            "~/.claude.json must not be writable"
        );
        assert_contains(
            &cfg.filesystem.deny_write,
            "/wb-test-home/u/.claude.json",
            "denyWrite",
        );
    }

    // --- denyWrite ---

    /// Defence in depth: redundant under the allowlist, but must survive a future
    /// widening of it.
    #[test]
    fn worst_claude_surfaces_are_also_explicitly_denied() {
        let cfg = build(&[], &[], None);
        for name in [
            "settings.json",
            "settings.local.json",
            "hooks",
            "plugins",
            "local",
            "skills",
            "agents",
            "commands",
            "shell-snapshots",
        ] {
            assert_contains(
                &cfg.filesystem.deny_write,
                &format!("/wb-test-home/u/.claude/{name}"),
                "denyWrite",
            );
        }
    }

    /// srt's mandatory denies only protect the cwd — verified against srt 0.0.76,
    /// where a sibling root's `.git/hooks` and `.claude/` were writable.
    #[test]
    fn project_escape_hatches_are_denied_per_project_root() {
        let cfg = build(&[], &[project("/repos/a"), project("/repos/b")], None);
        for root in ["/repos/a", "/repos/b"] {
            for rel in [
                ".git/hooks",
                ".git/config",
                ".git/modules",
                ".claude/settings.json",
                ".claude/settings.local.json",
                ".claude/hooks",
                ".claude/agents",
                ".claude/skills",
                ".claude/commands",
                ".mcp.json",
            ] {
                assert_contains(
                    &cfg.filesystem.deny_write,
                    &format!("{root}/{rel}"),
                    "denyWrite",
                );
            }
        }
    }

    // --- denyRead ---

    #[test]
    fn credential_stores_are_unreadable() {
        let cfg = build(&[], &[], None);
        for name in [
            ".ssh",
            ".aws",
            ".gnupg",
            ".netrc",
            ".config/gh",
            ".docker",
            ".kube",
        ] {
            assert_contains(
                &cfg.filesystem.deny_read,
                &format!("/wb-test-home/u/{name}"),
                "denyRead",
            );
        }
    }

    /// Holds the Trello API token.
    #[test]
    fn workbench_settings_file_is_unreadable() {
        let cfg = build(&[], &[], None);
        assert_contains(
            &cfg.filesystem.deny_read,
            "/wb-test-home/u/.workbench/settings.json",
            "denyRead",
        );
    }

    /// Claude cannot authenticate without its own OAuth token.
    #[test]
    fn claude_credentials_stay_readable() {
        let cfg = build(&[], &[], None);
        assert!(
            !cfg.filesystem
                .deny_read
                .iter()
                .any(|p| p.contains(".credentials.json")),
            "Claude's own credentials must stay readable: {:?}",
            cfg.filesystem.deny_read
        );
    }

    // --- paths ---

    /// Relying on srt's own tilde expansion would make the file's meaning depend
    /// on how srt resolves the home dir.
    #[test]
    fn no_path_is_emitted_with_a_tilde() {
        let cfg = build(&[], &[project("/repos/a")], None);
        for (label, list) in [
            ("allowWrite", &cfg.filesystem.allow_write),
            ("denyWrite", &cfg.filesystem.deny_write),
            ("denyRead", &cfg.filesystem.deny_read),
        ] {
            for path in list {
                assert!(
                    !path.starts_with('~'),
                    "{label} entry {path} must be absolute, not tilde-relative"
                );
            }
        }
    }

    /// srt matches on the real path, so an unresolved symlink grants nothing.
    /// This is why `/tmp` in `allowWrite` is a no-op on macOS.
    #[test]
    fn symlinked_paths_are_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // Both an existing symlink and a not-yet-created child resolve through it.
        assert_eq!(resolved(&link), resolved(&real));
        assert_eq!(
            resolved(&link.join("child.json")),
            format!("{}/child.json", resolved(&real))
        );
    }

    #[test]
    fn a_nonexistent_path_is_emitted_unchanged() {
        assert_eq!(
            resolved(Path::new("/definitely/not/here/x.json")),
            "/definitely/not/here/x.json"
        );
    }

    // --- network ---

    /// Removing a required host in the settings UI must not be able to break
    /// Claude, so the defaults are unioned in rather than replaced.
    #[test]
    fn required_domains_survive_a_user_list_that_omits_them() {
        let cfg = build(&["github.com"], &[], None);
        assert_contains(
            &cfg.network.allowed_domains,
            "api.anthropic.com",
            "allowedDomains",
        );
        assert_contains(&cfg.network.allowed_domains, "claude.ai", "allowedDomains");
        assert_contains(&cfg.network.allowed_domains, "github.com", "allowedDomains");
    }

    /// srt accepts a bare `host:port`, IP literal included — pinned because the
    /// hook bridge is reachable only through this entry shape.
    #[test]
    fn hook_socket_is_appended_as_a_bare_host_port() {
        let cfg = build(&[], &[], Some("127.0.0.1:51234"));
        assert_contains(
            &cfg.network.allowed_domains,
            "127.0.0.1:51234",
            "allowedDomains",
        );
        let entry = cfg
            .network
            .allowed_domains
            .iter()
            .find(|d| d.starts_with("127.0.0.1"))
            .expect("hook socket entry");
        assert_eq!(entry, "127.0.0.1:51234", "must stay a bare host:port");
    }

    #[test]
    fn local_binding_is_allowed_and_nothing_is_explicitly_denied() {
        let cfg = build(&[], &[], None);
        assert!(cfg.network.allow_local_binding);
        assert!(cfg.network.denied_domains.is_empty());
    }

    // --- dedupe ---

    #[test]
    fn duplicate_project_paths_are_collapsed() {
        let cfg = build(&[], &[project("/repos/a"), project("/repos/a")], None);
        let hits = cfg
            .filesystem
            .allow_write
            .iter()
            .filter(|p| *p == "/repos/a")
            .count();
        assert_eq!(hits, 1);
    }

    #[test]
    fn duplicate_domains_are_collapsed() {
        let cfg = build(&["github.com", "github.com"], &[], Some("github.com"));
        let hits = cfg
            .network
            .allowed_domains
            .iter()
            .filter(|d| *d == "github.com")
            .count();
        assert_eq!(hits, 1);
    }

    #[test]
    fn a_user_entry_duplicating_a_default_is_collapsed() {
        let cfg = build(&["api.anthropic.com"], &[], None);
        let hits = cfg
            .network
            .allowed_domains
            .iter()
            .filter(|d| *d == "api.anthropic.com")
            .count();
        assert_eq!(hits, 1);
    }

    // --- serialised shape ---

    /// srt refuses to run when a required key is absent, so the serialised shape
    /// must always carry all five arrays plus `allowLocalBinding`.
    #[test]
    fn serialises_every_key_srt_requires() {
        let json = serde_json::to_value(build(&[], &[], None)).expect("config should serialise");
        let fs = &json["filesystem"];
        let net = &json["network"];
        for key in ["allowWrite", "denyWrite", "denyRead"] {
            assert!(fs[key].is_array(), "filesystem.{key} must be an array");
        }
        for key in ["allowedDomains", "deniedDomains"] {
            assert!(net[key].is_array(), "network.{key} must be an array");
        }
        assert!(net["allowLocalBinding"].is_boolean());
    }

    #[test]
    fn defaults_cover_the_anthropic_api_and_auth_hosts() {
        let domains = default_allowed_domains();
        for expected in ["api.anthropic.com", "claude.ai", "platform.claude.com"] {
            assert_contains(&domains, expected, "the default allowlist");
        }
    }

    #[test]
    fn write_settings_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_settings_to(
            Path::new(HOME),
            dir.path(),
            &settings(&["github.com"]),
            &[project("/repos/a")],
            Some("127.0.0.1:9999"),
        )
        .unwrap();

        assert_eq!(path, dir.path().join(SETTINGS_FILE));
        let written: SandboxRuntimeConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_contains(
            &written.network.allowed_domains,
            "127.0.0.1:9999",
            "allowedDomains",
        );
        assert_contains(
            &written.network.allowed_domains,
            "api.anthropic.com",
            "allowedDomains",
        );
        assert_contains(&written.filesystem.allow_write, "/repos/a", "allowWrite");
        assert_contains(
            &written.filesystem.deny_write,
            "/repos/a/.git/hooks",
            "denyWrite",
        );
        assert_contains(
            &written.filesystem.deny_write,
            &resolved(dir.path()),
            "denyWrite",
        );
    }
}
