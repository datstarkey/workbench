/// Codex CLI configuration management.
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use crate::paths;

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
        "Workbench will update your Codex config (~/.codex/config/config.toml) to add CLAUDE.md as a project doc fallback and install a notify bridge script.".to_string()
    } else {
        String::new()
    };

    crate::types::IntegrationStatus {
        needs_changes,
        description,
    }
}

/// Ensure Codex config has project_doc_fallback_filenames and the notify bridge.
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
