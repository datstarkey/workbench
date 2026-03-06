/// Codex CLI configuration management.
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::{self, remove_path_if_exists};

#[cfg(not(windows))]
const WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME: &str = "workbench-codex-notify-bridge.sh";
#[cfg(windows)]
const WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME: &str = "workbench-codex-notify-bridge.ps1";

fn workbench_codex_notify_script_path() -> PathBuf {
    paths::codex_config_dir().join(WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME)
}

#[cfg(not(windows))]
fn workbench_codex_notify_script_body() -> &'static str {
    "#!/usr/bin/env bash\n\
SOCKET=\"${WORKBENCH_HOOK_SOCKET}\"\n\
PANE_ID=\"${WORKBENCH_PANE_ID}\"\n\
[[ -z \"$SOCKET\" || -z \"$PANE_ID\" || -z \"$1\" ]] && exit 0\n\
PAYLOAD=$(printf '%s' \"$1\" | tr -d '\\n\\r')\n\
IFS=: read -r HOST PORT <<< \"$SOCKET\"\n\
exec 3<>/dev/tcp/\"$HOST\"/\"$PORT\" 2>/dev/null || exit 0\n\
printf '{\"pane_id\":\"%s\",\"codex\":%s}\\n' \"$PANE_ID\" \"$PAYLOAD\" >&3\n"
}

#[cfg(windows)]
fn workbench_codex_notify_script_body() -> &'static str {
    "$socket = $env:WORKBENCH_HOOK_SOCKET\n\
$paneId = $env:WORKBENCH_PANE_ID\n\
if (-not $socket -or -not $paneId -or $args.Count -eq 0) { exit 0 }\n\
$payload = ($args[0] -replace '\\s+', ' ').Trim()\n\
$msg = [Text.Encoding]::UTF8.GetBytes(\"{`\"pane_id`\":`\"$paneId`\",`\"codex`\":$payload}`n\")\n\
try {\n\
    $parts = $socket -split ':'\n\
    $tcp = [Net.Sockets.TcpClient]::new($parts[0], [int]$parts[1])\n\
    $tcp.GetStream().Write($msg, 0, $msg.Length)\n\
    $tcp.Close()\n\
} catch { }\n"
}

fn ensure_workbench_codex_notify_script() -> Result<PathBuf> {
    paths::ensure_script(
        &workbench_codex_notify_script_path(),
        workbench_codex_notify_script_body(),
    )
}

fn toml_escape_str(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Link a skill directory into the agents tree.
/// Unix: symlink. Windows: tries symlink, falls back to copy when
/// Developer Mode / symlink privileges are unavailable.
fn link_or_copy_skill(source: &Path, dest: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, dest)?;
        return Ok(());
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(source, dest).is_ok() {
            return Ok(());
        }
        paths::copy_dir_skip_symlinks(source, dest)
    }
}

/// Preserve a filesystem entry (file, directory, or symlink) by
/// recreating it at `dst`.
fn preserve_entry(src: &Path, dst: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(src)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, dst)?;
        #[cfg(windows)]
        {
            if fs::metadata(&target).map(|m| m.is_dir()).unwrap_or(true) {
                std::os::windows::fs::symlink_dir(&target, dst)?;
            } else {
                std::os::windows::fs::symlink_file(&target, dst)?;
            }
        }
    } else if meta.is_dir() {
        paths::copy_dir_skip_symlinks(src, dst)?;
    } else if meta.is_file() {
        paths::copy_file(src, dst)?;
    }
    Ok(())
}

/// Sync skills from `source_skills` into `agents_dir/skills` using symlinks
/// (or copies on Windows). Non-Claude entries are preserved. Uses a staged
/// swap so a failure mid-sync never leaves a partially updated tree.
fn sync_skills(source_skills: &Path, agents_dir: &Path) -> Result<()> {
    if !source_skills.is_dir() {
        return Ok(());
    }

    fs::create_dir_all(agents_dir)?;

    let destination = agents_dir.join("skills");
    let staging = agents_dir.join("skills.workbench-staging");
    let backup = agents_dir.join("skills.workbench-backup");

    // Clean up leftovers from a previous failed sync.
    remove_path_if_exists(&staging)?;
    remove_path_if_exists(&backup)?;
    fs::create_dir_all(&staging)?;

    // Phase 1: Link each valid Claude skill into staging.
    let mut claude_skill_names = HashSet::new();
    for entry in fs::read_dir(source_skills)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let entry_path = entry.path();
        let resolved = match fs::canonicalize(&entry_path) {
            Ok(p) => p,
            Err(_) => continue, // broken symlink in source
        };
        if !resolved.is_dir() || !resolved.join("SKILL.md").is_file() {
            continue;
        }
        claude_skill_names.insert(entry.file_name());
        link_or_copy_skill(&entry_path, &staging.join(entry.file_name()))?;
    }

    // Phase 2: Preserve non-Claude entries from the current destination.
    if destination.is_dir() {
        if let Ok(entries) = fs::read_dir(&destination) {
            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let name = entry.file_name();
                if claude_skill_names.contains(&name) {
                    continue; // replaced by phase 1
                }
                let src_path = destination.join(&name);
                // Skip stale symlinks that point into source_skills.
                if let Ok(meta) = fs::symlink_metadata(&src_path) {
                    if meta.file_type().is_symlink() {
                        if let Ok(target) = fs::read_link(&src_path) {
                            if target.starts_with(source_skills) {
                                continue;
                            }
                        }
                    }
                }
                // Best-effort preserve — don't fail the whole sync for one entry.
                let _ = preserve_entry(&src_path, &staging.join(&name));
            }
        }
    }

    // Phase 3: Atomic swap.
    if fs::symlink_metadata(&destination).is_ok() {
        fs::rename(&destination, &backup)?;
    }
    match fs::rename(&staging, &destination) {
        Ok(()) => {
            remove_path_if_exists(&backup)?;
            Ok(())
        }
        Err(err) => {
            if fs::symlink_metadata(&backup).is_ok() {
                let _ = fs::rename(&backup, &destination);
            }
            Err(err.into())
        }
    }
}

fn sync_claude_skills_into_agents() -> Result<()> {
    sync_skills(
        &paths::claude_user_dir().join("skills"),
        &paths::agents_dir(),
    )
}

pub(crate) fn ensure_codex_notify_config(content: &str, script_path: &str) -> (String, bool) {
    let escaped_path = toml_escape_str(script_path);
    #[cfg(not(windows))]
    let notify_line = format!("notify = [\"bash\", \"{}\"]", escaped_path);
    #[cfg(windows)]
    let notify_line = format!(
        "notify = [\"powershell.exe\", \"-ExecutionPolicy\", \"Bypass\", \"-File\", \"{}\"]",
        escaped_path
    );

    if content.contains(script_path) {
        return (content.to_string(), false);
    }

    let mut replaced = false;
    let had_trailing_newline = content.ends_with('\n');
    let mut lines = Vec::new();
    for line in content.lines() {
        let is_top_level_notify = line.trim_start() == line && line.starts_with("notify =");
        if !replaced && is_top_level_notify {
            lines.push(notify_line.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if replaced {
        let mut updated = lines.join("\n");
        if had_trailing_newline {
            updated.push('\n');
        }
        return (updated, true);
    }

    let mut updated = content.to_string();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&notify_line);
    updated.push('\n');
    (updated, true)
}

pub fn check_codex_config_status() -> crate::types::IntegrationStatus {
    let codex_dir = paths::codex_config_dir();
    let config_path = codex_dir.join("config.toml");
    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path).unwrap_or_default()
    } else {
        String::new()
    };

    let has_fallback = content.contains("project_doc_fallback_filenames");
    let script_path = workbench_codex_notify_script_path();
    let script_exists = script_path.exists();
    let has_notify = script_exists && content.contains(&script_path.to_string_lossy().to_string());

    let needs_changes = !has_fallback || !has_notify;
    let description = if needs_changes {
        "Workbench will update your Codex config (~/.codex/config/config.toml) to add CLAUDE.md as a project doc fallback, install a notify bridge script, and symlink Claude skills into Codex agents.".to_string()
    } else {
        String::new()
    };

    crate::types::IntegrationStatus {
        needs_changes,
        description,
    }
}

/// Ensure Codex config has project_doc_fallback_filenames and skills symlinks.
pub fn ensure_codex_config() -> Result<()> {
    let codex_dir = paths::codex_config_dir();
    fs::create_dir_all(&codex_dir)?;

    // Ensure config.toml has project_doc_fallback_filenames
    let config_path = codex_dir.join("config.toml");
    let content = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    let mut updated_content = content.clone();
    let mut changed = false;

    if !updated_content.contains("project_doc_fallback_filenames") {
        let addition = if updated_content.is_empty() || updated_content.ends_with('\n') {
            "project_doc_fallback_filenames = [\"CLAUDE.md\"]\n"
        } else {
            "\nproject_doc_fallback_filenames = [\"CLAUDE.md\"]\n"
        };
        updated_content.push_str(addition);
        changed = true;
    }

    let script_path = ensure_workbench_codex_notify_script()?;
    let script_path_str = script_path.to_string_lossy().to_string();
    let (with_notify, notify_changed) =
        ensure_codex_notify_config(&updated_content, &script_path_str);
    if notify_changed {
        updated_content = with_notify;
        changed = true;
    }

    if changed {
        paths::atomic_write(&config_path, &updated_content)?;
    }

    // Ensure Codex can discover user skills from Claude skill directories.
    sync_claude_skills_into_agents()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ensure_codex_notify_config
    // -----------------------------------------------------------------------

    #[cfg(not(windows))]
    #[test]
    fn notify_config_empty_content() {
        let (result, changed) = ensure_codex_notify_config("", "/path/to/script.sh");
        assert!(changed);
        assert!(result.contains("notify = [\"bash\", \"/path/to/script.sh\"]"));
        assert!(result.ends_with('\n'));
    }

    #[cfg(not(windows))]
    #[test]
    fn notify_config_already_contains_script_path() {
        let content = "notify = [\"bash\", \"/path/to/script.sh\"]\n";
        let (result, changed) = ensure_codex_notify_config(content, "/path/to/script.sh");
        assert!(!changed);
        assert_eq!(result, content);
    }

    #[cfg(not(windows))]
    #[test]
    fn notify_config_replaces_existing_notify_line() {
        let content = "some_key = true\nnotify = [\"old\", \"command\"]\nother = 1\n";
        let (result, changed) = ensure_codex_notify_config(content, "/path/to/script.sh");
        assert!(changed);
        assert!(result.contains("notify = [\"bash\", \"/path/to/script.sh\"]"));
        assert!(!result.contains("old"));
        assert!(result.contains("some_key = true"));
        assert!(result.contains("other = 1"));
    }

    #[cfg(not(windows))]
    #[test]
    fn notify_config_appends_when_no_notify() {
        let content = "some_key = true\nother = 1\n";
        let (result, changed) = ensure_codex_notify_config(content, "/path/to/script.sh");
        assert!(changed);
        assert!(result.contains("notify = [\"bash\", \"/path/to/script.sh\"]"));
        assert!(result.ends_with('\n'));
    }

    #[test]
    fn notify_config_preserves_trailing_newline() {
        let content = "notify = [\"old\"]\n";
        let (result, _) = ensure_codex_notify_config(content, "/new/script.sh");
        assert!(result.ends_with('\n'));
    }

    #[cfg(not(windows))]
    #[test]
    fn notify_config_adds_newline_before_appending() {
        let content = "key = value";
        let (result, changed) = ensure_codex_notify_config(content, "/path/to/script.sh");
        assert!(changed);
        assert!(result.starts_with("key = value\n"));
        assert!(result.contains("notify = [\"bash\", \"/path/to/script.sh\"]"));
    }

    #[cfg(windows)]
    #[test]
    fn notify_config_windows_uses_powershell() {
        let (result, changed) =
            ensure_codex_notify_config("", "C:\\Users\\test\\.codex\\script.ps1");
        assert!(changed);
        assert!(result.contains("powershell.exe"));
        assert!(result.contains("-ExecutionPolicy"));
        assert!(result.contains("Bypass"));
        assert!(result.contains("script.ps1"));
    }

    // -----------------------------------------------------------------------
    // sync_skills
    // -----------------------------------------------------------------------

    #[cfg(unix)]
    mod sync_skills_tests {
        use super::super::*;
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        fn create_skill(base: &std::path::Path, name: &str) {
            let dir = base.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("SKILL.md"), "# Test skill").unwrap();
        }

        #[test]
        fn creates_symlinks() {
            let tmp = tempdir().unwrap();
            let source = tmp.path().join("skills");
            let agents = tmp.path().join("agents");
            fs::create_dir_all(&source).unwrap();
            create_skill(&source, "skill-a");
            create_skill(&source, "skill-b");

            sync_skills(&source, &agents).unwrap();

            let dest = agents.join("skills");
            let meta_a = fs::symlink_metadata(dest.join("skill-a")).unwrap();
            let meta_b = fs::symlink_metadata(dest.join("skill-b")).unwrap();
            assert!(meta_a.file_type().is_symlink());
            assert!(meta_b.file_type().is_symlink());
            assert_eq!(
                fs::read_link(dest.join("skill-a")).unwrap(),
                source.join("skill-a")
            );
        }

        #[test]
        fn noop_when_already_correct() {
            let tmp = tempdir().unwrap();
            let source = tmp.path().join("skills");
            let agents = tmp.path().join("agents");
            fs::create_dir_all(&source).unwrap();
            create_skill(&source, "skill-a");

            sync_skills(&source, &agents).unwrap();
            // Run again — should succeed without error.
            sync_skills(&source, &agents).unwrap();

            let dest = agents.join("skills");
            assert!(fs::symlink_metadata(dest.join("skill-a"))
                .unwrap()
                .file_type()
                .is_symlink());
        }

        #[test]
        fn replaces_broken_symlink() {
            let tmp = tempdir().unwrap();
            let source = tmp.path().join("skills");
            let agents = tmp.path().join("agents");
            let dest = agents.join("skills");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&dest).unwrap();
            create_skill(&source, "skill-a");
            // Plant a broken symlink at the destination.
            symlink(tmp.path().join("nonexistent"), dest.join("skill-a")).unwrap();
            assert!(!dest.join("skill-a").exists()); // broken

            sync_skills(&source, &agents).unwrap();

            assert!(dest.join("skill-a").exists()); // now valid
            assert_eq!(
                fs::read_link(dest.join("skill-a")).unwrap(),
                source.join("skill-a")
            );
        }

        #[test]
        fn cleans_stale_symlinks() {
            let tmp = tempdir().unwrap();
            let source = tmp.path().join("skills");
            let agents = tmp.path().join("agents");
            fs::create_dir_all(&source).unwrap();
            create_skill(&source, "skill-a");
            create_skill(&source, "skill-b");

            sync_skills(&source, &agents).unwrap();

            let dest = agents.join("skills");
            assert!(dest.join("skill-b").exists());

            // Remove skill-b from source, re-sync.
            fs::remove_dir_all(source.join("skill-b")).unwrap();
            sync_skills(&source, &agents).unwrap();

            assert!(dest.join("skill-a").exists());
            assert!(!fs::symlink_metadata(dest.join("skill-b")).is_ok());
        }

        #[test]
        fn preserves_non_claude_entries() {
            let tmp = tempdir().unwrap();
            let source = tmp.path().join("skills");
            let agents = tmp.path().join("agents");
            let dest = agents.join("skills");
            fs::create_dir_all(&source).unwrap();
            create_skill(&source, "skill-a");

            // Pre-populate destination with a non-Claude entry.
            fs::create_dir_all(dest.join("user-skill")).unwrap();
            fs::write(dest.join("user-skill").join("SKILL.md"), "# User").unwrap();

            sync_skills(&source, &agents).unwrap();

            assert!(dest.join("skill-a").exists());
            assert!(dest.join("user-skill").join("SKILL.md").exists());
            // user-skill should remain a real directory, not a symlink.
            assert!(!fs::symlink_metadata(dest.join("user-skill"))
                .unwrap()
                .file_type()
                .is_symlink());
        }

        #[test]
        fn replaces_old_copy_with_symlink() {
            let tmp = tempdir().unwrap();
            let source = tmp.path().join("skills");
            let agents = tmp.path().join("agents");
            let dest = agents.join("skills");
            fs::create_dir_all(&source).unwrap();
            create_skill(&source, "skill-a");

            // Simulate old deep-copy sync.
            fs::create_dir_all(dest.join("skill-a")).unwrap();
            fs::write(dest.join("skill-a").join("SKILL.md"), "# Old copy").unwrap();

            sync_skills(&source, &agents).unwrap();

            assert!(fs::symlink_metadata(dest.join("skill-a"))
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(
                fs::read_link(dest.join("skill-a")).unwrap(),
                source.join("skill-a")
            );
        }

        #[test]
        fn noop_when_no_source() {
            let tmp = tempdir().unwrap();
            let source = tmp.path().join("skills"); // does not exist
            let agents = tmp.path().join("agents");

            assert!(sync_skills(&source, &agents).is_ok());
            assert!(!agents.join("skills").exists());
        }
    }
}
