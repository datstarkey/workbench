//! The `workbench` Claude Code plugin (`plugins/workbench`, also installable
//! from this repo's marketplace) reports session activity to the desktop's
//! hook bridge. Workbench loads an embedded copy into every process it starts
//! through `CLAUDE_CODE_PLUGIN_DIRS`; Claude Code loads one plugin per name, so
//! an installed copy beside it doesn't report twice.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

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
}
