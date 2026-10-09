//! The `workbench` Claude Code plugin (`plugins/workbench`) reports session
//! activity to the desktop's hook bridge. Workbench loads an embedded copy into
//! every process it starts through `CLAUDE_CODE_PLUGIN_DIRS`, the only route:
//! the repo used to be a marketplace too, and that frozen install is disabled
//! at startup (`disable_marketplace_install`).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

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

const MARKETPLACE_PLUGIN: &str = "workbench@workbench";
const MARKETPLACE_REPOS: [&str; 2] = ["datstarkey/workbench", "starkey-digital/workbench"];

/// Drops `enabledPlugins["workbench@workbench"]` and a `workbench` marketplace
/// sourced from this repo. Plugin caches stay: Claude Code owns them.
fn remove_marketplace_entries(settings: &mut Value) -> bool {
    let mut changed = settings
        .get_mut("enabledPlugins")
        .and_then(Value::as_object_mut)
        .is_some_and(|plugins| plugins.remove(MARKETPLACE_PLUGIN).is_some());
    if let Some(markets) = settings
        .get_mut("extraKnownMarketplaces")
        .and_then(Value::as_object_mut)
    {
        let ours = markets.get("workbench").is_some_and(|m| {
            let source = m.get("source").map(Value::to_string).unwrap_or_default();
            let source = source.to_ascii_lowercase();
            MARKETPLACE_REPOS.iter().any(|repo| source.contains(repo))
        });
        if ours {
            markets.remove("workbench");
            changed = true;
        }
    }
    changed
}

fn disable_marketplace_install_in(claude_dir: &Path) -> Result<bool> {
    let path = claude_dir.join("settings.json");
    let Ok(raw) = fs::read_to_string(&path) else {
        return Ok(false);
    };
    let mut settings: Value = serde_json::from_str(&raw)?;
    if !remove_marketplace_entries(&mut settings) {
        return Ok(false);
    }
    paths::atomic_write(&path, &serde_json::to_string_pretty(&settings)?)?;
    Ok(true)
}

/// Disables a marketplace install of the plugin in every Claude account, so the
/// injected copy is the only one. One account's failure doesn't stop the
/// others; the first error is returned.
pub fn disable_marketplace_install() -> Result<()> {
    let mut first_error = None;
    for (_, dir) in crate::claude_accounts::saved_config_dirs() {
        match disable_marketplace_install_in(&dir) {
            Ok(true) => log::info!(
                "disabled the marketplace-installed workbench plugin in {}",
                dir.display()
            ),
            Ok(false) => {}
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
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
    fn disables_only_the_marketplace_install() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let settings = serde_json::json!({
            "model": "opus",
            "enabledPlugins": { "workbench@workbench": true, "other@market": true },
            "extraKnownMarketplaces": {
                "workbench": { "source": { "source": "github", "repo": "DatStarkey/workbench" } },
                "market": { "source": { "source": "github", "repo": "someone/market" } }
            },
            "hooks": { "Stop": [] }
        });
        fs::write(&path, settings.to_string()).unwrap();

        assert!(disable_marketplace_install_in(tmp.path()).unwrap());
        let saved: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({
                "model": "opus",
                "enabledPlugins": { "other@market": true },
                "extraKnownMarketplaces": {
                    "market": { "source": { "source": "github", "repo": "someone/market" } }
                },
                "hooks": { "Stop": [] }
            })
        );
    }

    #[test]
    fn keeps_a_workbench_marketplace_from_elsewhere() {
        let mut other = serde_json::json!({
            "extraKnownMarketplaces": {
                "workbench": { "source": { "source": "url", "url": "https://example.com/workbench" } }
            }
        });
        let before = other.clone();
        assert!(!remove_marketplace_entries(&mut other));
        assert_eq!(other, before);

        let mut ours = serde_json::json!({
            "extraKnownMarketplaces": {
                "workbench": { "source": { "source": "url", "url": "https://github.com/starkey-digital/workbench" } }
            }
        });
        assert!(remove_marketplace_entries(&mut ours));
        assert_eq!(ours, serde_json::json!({ "extraKnownMarketplaces": {} }));
    }

    #[test]
    fn never_writes_when_nothing_to_remove() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!disable_marketplace_install_in(tmp.path()).unwrap());
        assert!(!tmp.path().join("settings.json").exists());

        let path = tmp.path().join("settings.json");
        let raw = r#"{"enabledPlugins":{"other@market":true},  "model":"opus"}"#;
        fs::write(&path, raw).unwrap();
        assert!(!disable_marketplace_install_in(tmp.path()).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    }
}
