/// Codex CLI configuration management.
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::paths::{self, copy_dir_follow_symlinks, remove_path_if_exists};

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

fn sync_claude_skills_into_agents() -> Result<()> {
    let source_skills = paths::claude_user_dir().join("skills");
    if !source_skills.is_dir() {
        return Ok(());
    }

    let agents_dir = paths::agents_dir();
    fs::create_dir_all(&agents_dir)?;

    let destination_skills = agents_dir.join("skills");
    let staging_skills = agents_dir.join("skills.workbench-staging");
    let backup_skills = agents_dir.join("skills.workbench-backup");

    remove_path_if_exists(&staging_skills)?;
    remove_path_if_exists(&backup_skills)?;
    fs::create_dir_all(&staging_skills)?;

    // Preserve existing ~/.agents/skills entries that are not mirrored from ~/.claude/skills.
    if destination_skills.exists() {
        let mut visited = HashSet::new();
        copy_dir_follow_symlinks(&destination_skills, &staging_skills, &mut visited)?;
    }

    for entry in fs::read_dir(&source_skills)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let entry_path = entry.path();
        let entry_meta = match fs::symlink_metadata(&entry_path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };

        let resolved = if entry_meta.file_type().is_symlink() {
            match fs::canonicalize(&entry_path) {
                Ok(path) => path,
                Err(_) => continue,
            }
        } else {
            entry_path
        };

        if !resolved.is_dir() || !resolved.join("SKILL.md").is_file() {
            continue;
        }

        let dst = staging_skills.join(entry.file_name());
        remove_path_if_exists(&dst)?;
        let mut visited = HashSet::new();
        copy_dir_follow_symlinks(&resolved, &dst, &mut visited)?;
    }

    if destination_skills.exists() {
        fs::rename(&destination_skills, &backup_skills)?;
    }

    match fs::rename(&staging_skills, &destination_skills) {
        Ok(()) => {
            remove_path_if_exists(&backup_skills)?;
            Ok(())
        }
        Err(err) => {
            if backup_skills.exists() {
                let _ = fs::rename(&backup_skills, &destination_skills);
            }
            Err(err.into())
        }
    }
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
        "Workbench will update your Codex config (~/.codex/config/config.toml) to add CLAUDE.md as a project doc fallback, install a notify bridge script, and sync Claude skills to Codex agents.".to_string()
    } else {
        String::new()
    };

    crate::types::IntegrationStatus {
        needs_changes,
        description,
    }
}

/// Ensure Codex config has project_doc_fallback_filenames and skills symlink.
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
}
