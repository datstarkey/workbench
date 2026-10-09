//! The `workbench` Claude Code plugin (`plugins/workbench`) reports session
//! activity to the desktop's hook bridge. Workbench loads an embedded copy into
//! every process it starts through `CLAUDE_CODE_PLUGIN_DIRS`, the only route:
//! the repo used to be a marketplace too, and that frozen install is disabled
//! at startup (`disable_marketplace_install`).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Result;
use serde_json::Value;

use crate::paths;

pub const PLUGIN_DIRS_ENV: &str = "CLAUDE_CODE_PLUGIN_DIRS";

const FILES: &[(&str, &str)] = &[
    (
        ".claude-plugin/plugin.json",
        include_str!("../../../plugins/workbench/.claude-plugin/plugin.json"),
    ),
    (
        "hooks/hooks.json",
        include_str!("../../../plugins/workbench/hooks/hooks.json"),
    ),
    (
        "hooks/register.ts",
        include_str!("../../../plugins/workbench/hooks/register.ts"),
    ),
    (
        "hooks/chat.ts",
        include_str!("../../../plugins/workbench/hooks/chat.ts"),
    ),
    (
        "hooks/link.ts",
        include_str!("../../../plugins/workbench/hooks/link.ts"),
    ),
    (
        "hooks/lines.ts",
        include_str!("../../../plugins/workbench/hooks/lines.ts"),
    ),
    (
        "hooks/jobs.ts",
        include_str!("../../../plugins/workbench/hooks/jobs.ts"),
    ),
    (
        "hooks/asks.ts",
        include_str!("../../../plugins/workbench/hooks/asks.ts"),
    ),
    (
        "hooks/titles.ts",
        include_str!("../../../plugins/workbench/hooks/titles.ts"),
    ),
];

fn ensure_in(dir: &Path) -> Result<()> {
    for (rel, body) in FILES {
        let path = dir.join(rel);
        if fs::read_to_string(&path).ok().as_deref() != Some(*body) {
            paths::atomic_write(&path, body)?;
        }
    }
    Ok(())
}

/// Checked on every spawn (three small reads), so a failed write or a file
/// another Workbench build replaced is repaired on the next one.
fn plugin_dir() -> Option<PathBuf> {
    let dir = paths::workbench_config_dir()
        .join("claude-plugin")
        .join("workbench");
    ensure_in(&dir)
        .map_err(|e| log::warn!("[claude_plugin] Failed to write the Workbench plugin: {e}"))
        .ok()
        .map(|()| dir)
}

/// A Workbench plugin copy (ours, or another build's: a dev build started
/// from the installed app's terminal inherits its dir).
fn is_workbench_dir(dir: &Path) -> bool {
    dir.ends_with(Path::new("claude-plugin").join("workbench"))
}

/// `dirs` without any Workbench plugin copy; `None` when nothing else is left.
pub fn without_workbench_dirs(dirs: &std::ffi::OsStr) -> Option<OsString> {
    let kept: Vec<PathBuf> = std::env::split_paths(dirs)
        .filter(|d| !d.as_os_str().is_empty() && !is_workbench_dir(d))
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

/// Ours first, so it wins Claude Code's one-plugin-per-name pick, and no other
/// Workbench copy.
fn with_dir(existing: Option<OsString>, dir: &Path) -> Option<OsString> {
    let mut dirs = vec![dir.to_path_buf()];
    if let Some(rest) = existing.as_deref().and_then(without_workbench_dirs) {
        dirs.extend(std::env::split_paths(&rest));
    }
    std::env::join_paths(dirs).ok()
}

/// `CLAUDE_CODE_PLUGIN_DIRS` for a process Workbench starts: the plugin, then
/// this process's other dirs. `None` if the plugin couldn't be written.
pub fn plugin_dirs_env() -> Option<OsString> {
    with_dir(std::env::var_os(PLUGIN_DIRS_ENV), &plugin_dir()?)
}

const MARKETPLACE: &str = "workbench";
const MARKETPLACE_PLUGIN: &str = "workbench@workbench";
const MARKETPLACE_REPOS: [&str; 2] = ["datstarkey/workbench", "starkey-digital/workbench"];

/// `owner/name` of a GitHub marketplace source: its `repo`, or a `url` such as
/// `https://github.com/owner/name.git` or `git@github.com:owner/name`.
fn github_repo(source: &Value) -> Option<String> {
    let raw = source.get("repo").or_else(|| source.get("url"))?.as_str()?;
    let lower = raw.trim().to_ascii_lowercase();
    let repo = [
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|prefix| lower.strip_prefix(prefix))
    .unwrap_or(&lower)
    .trim_end_matches('/');
    Some(repo.strip_suffix(".git").unwrap_or(repo).to_string())
}

fn is_our_source(source: &Value) -> bool {
    github_repo(source).is_some_and(|repo| MARKETPLACE_REPOS.contains(&repo.as_str()))
}

/// Read-only: whether Claude Code's marketplace registry in `claude_dir` has
/// this repo as `workbench`.
fn has_our_marketplace(claude_dir: &Path) -> bool {
    fs::read_to_string(claude_dir.join("plugins").join("known_marketplaces.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|known| known.get(MARKETPLACE)?.get("source").cloned())
        .is_some_and(|source| is_our_source(&source))
}

fn run_claude(claude: &Path, account_dir: Option<&Path>, cwd: &Path, args: &[&str]) -> bool {
    let mut cmd = crate::shell::command(claude);
    cmd.args(args).current_dir(cwd).stdin(Stdio::null());
    match account_dir {
        Some(dir) => cmd.env(crate::claude_accounts::CONFIG_DIR_ENV, dir),
        None => cmd.env_remove(crate::claude_accounts::CONFIG_DIR_ENV),
    };
    match cmd.output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            log::warn!(
                "`claude {}` failed ({}): {}",
                args.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
            false
        }
        Err(e) => {
            log::warn!("couldn't run `claude {}`: {e}", args.join(" "));
            false
        }
    }
}

/// Uninstalls a marketplace install of the plugin and drops the marketplace,
/// through Claude Code's CLI (it owns those files and every settings scope),
/// only when the registry says the `workbench` marketplace is this repo.
fn disable_marketplace_install_in(claude: &Path, account_dir: Option<&Path>, claude_dir: &Path) {
    if !has_our_marketplace(claude_dir) {
        return;
    }
    let uninstalled = run_claude(
        claude,
        account_dir,
        claude_dir,
        &["plugin", "uninstall", MARKETPLACE_PLUGIN],
    );
    let removed = run_claude(
        claude,
        account_dir,
        claude_dir,
        &["plugin", "marketplace", "remove", MARKETPLACE],
    );
    if uninstalled || removed {
        log::info!(
            "removed the marketplace-installed workbench plugin from {}",
            claude_dir.display()
        );
    }
}

/// In the background, removes a marketplace install of the plugin (frozen at
/// its version, colliding with the injected copy) from each of `dirs`, as
/// [`crate::claude_accounts::config_dirs`] lists them.
pub fn spawn_disable_marketplace_install(dirs: Vec<(Option<String>, PathBuf)>) {
    std::thread::spawn(move || disable_marketplace_installs(dirs));
}

/// Startup entry point (desktop and standalone server): every saved account.
pub fn disable_marketplace_install_at_startup() {
    std::thread::spawn(
        || disable_marketplace_installs(crate::claude_accounts::saved_config_dirs()),
    );
}

fn disable_marketplace_installs(dirs: Vec<(Option<String>, PathBuf)>) {
    let claude = crate::claude_accounts::claude_binary();
    for (id, dir) in dirs {
        disable_marketplace_install_in(&claude, id.is_some().then_some(dir.as_path()), &dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_plugin_and_rewrites_a_stale_file() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_in(tmp.path()).unwrap();
        let module = tmp.path().join("hooks/register.ts");
        assert!(fs::read_to_string(&module)
            .unwrap()
            .contains("WORKBENCH_HOOK_SOCKET"));

        fs::write(&module, "stale").unwrap();
        ensure_in(tmp.path()).unwrap();
        assert_ne!(fs::read_to_string(&module).unwrap(), "stale");
        assert!(tmp.path().join(".claude-plugin/plugin.json").exists());
    }

    #[test]
    fn embeds_every_hooks_file() {
        let hooks = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/workbench/hooks");
        for entry in fs::read_dir(hooks).unwrap() {
            let rel = format!("hooks/{}", entry.unwrap().file_name().to_string_lossy());
            assert!(
                FILES.iter().any(|(r, _)| *r == rel),
                "{rel} is missing from FILES"
            );
        }
    }

    #[test]
    fn every_relative_import_is_embedded() {
        for (rel, body) in FILES.iter().filter(|(r, _)| r.ends_with(".ts")) {
            let dir = Path::new(rel).parent().unwrap();
            let specs = body.lines().filter_map(|l| {
                let rest = l.split_once(" from ")?.1.trim_start();
                let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
                rest[1..].split(quote).next()
            });
            for spec in specs.filter(|s| s.starts_with("./")) {
                let target = format!("{}/{}.ts", dir.display(), &spec[2..]);
                assert!(
                    FILES.iter().any(|(r, _)| *r == target),
                    "{rel} imports {spec}, which is missing from FILES"
                );
            }
        }
    }

    #[test]
    fn puts_the_plugin_first_and_drops_another_workbench_copy() {
        let dir = Path::new("/wb/claude-plugin/workbench");
        let user = std::env::join_paths([
            Path::new("/installed/.workbench/claude-plugin/workbench"),
            Path::new("/mine"),
        ])
        .unwrap();
        let joined = with_dir(Some(user), dir).unwrap();
        let dirs: Vec<PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(dirs, [dir.to_path_buf(), PathBuf::from("/mine")]);

        let again = with_dir(Some(joined), dir).unwrap();
        assert_eq!(std::env::split_paths(&again).count(), 2);
        assert_eq!(
            with_dir(None, dir).unwrap(),
            OsString::from(dir.as_os_str())
        );
        assert_eq!(
            without_workbench_dirs(dir.as_os_str()),
            None,
            "nothing of the user's"
        );
    }

    #[test]
    fn matches_only_this_repo_as_a_marketplace_source() {
        let ours = [
            serde_json::json!({ "source": "github", "repo": "datstarkey/workbench" }),
            serde_json::json!({ "source": "github", "repo": "Starkey-Digital/Workbench" }),
            serde_json::json!({ "source": "url", "url": "https://github.com/starkey-digital/workbench.git" }),
            serde_json::json!({ "source": "url", "url": "https://github.com/datstarkey/workbench/" }),
            serde_json::json!({ "source": "url", "url": "git@github.com:datstarkey/workbench.git" }),
        ];
        for source in &ours {
            assert!(is_our_source(source), "{source}");
        }
        let others = [
            serde_json::json!({ "source": "github", "repo": "datstarkey/workbench-plugins" }),
            serde_json::json!({ "source": "github", "repo": "someone/workbench" }),
            serde_json::json!({ "source": "github", "repo": "fork-of-datstarkey/workbench" }),
            serde_json::json!({ "source": "url", "url": "https://example.com/datstarkey/workbench" }),
            serde_json::json!({ "source": "url", "url": "https://github.com/datstarkey/workbench/tree/main" }),
            serde_json::json!({ "source": "directory", "path": "/src/datstarkey/workbench" }),
        ];
        for source in &others {
            assert!(!is_our_source(source), "{source}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn removes_the_marketplace_install_with_the_cli_only_when_it_is_ours() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("argv.log");
        let claude = tmp.path().join("claude");
        fs::write(
            &claude,
            format!(
                "#!/bin/sh\necho \"${{CLAUDE_CONFIG_DIR:-default}}|$*\" >> '{}'\n",
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();

        let registry = |name: &str, repo: &str| {
            let dir = tmp.path().join(name);
            fs::create_dir_all(dir.join("plugins")).unwrap();
            let known = serde_json::json!({
                "workbench": { "source": { "source": "github", "repo": repo } },
                "other": { "source": { "source": "github", "repo": "someone/other" } }
            });
            fs::write(
                dir.join("plugins").join("known_marketplaces.json"),
                known.to_string(),
            )
            .unwrap();
            dir
        };
        let default = registry("default", "datstarkey/workbench");
        let account = registry("account", "Starkey-Digital/workbench");
        let fork = registry("fork", "someone/workbench");
        let empty = tmp.path().join("empty");

        disable_marketplace_install_in(&claude, None, &default);
        disable_marketplace_install_in(&claude, Some(&account), &account);
        disable_marketplace_install_in(&claude, Some(&fork), &fork);
        disable_marketplace_install_in(&claude, Some(&empty), &empty);

        let a = account.display();
        assert_eq!(
            fs::read_to_string(&log).unwrap(),
            format!(
                "default|plugin uninstall workbench@workbench\n\
                 default|plugin marketplace remove workbench\n\
                 {a}|plugin uninstall workbench@workbench\n\
                 {a}|plugin marketplace remove workbench\n"
            )
        );
    }
}
