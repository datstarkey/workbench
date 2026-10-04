use anyhow::{bail, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::types::{HookScriptInfo, PluginInfo, SkillInfo};

pub(crate) fn settings_path(scope: &str, project_path: Option<&str>) -> Result<PathBuf> {
    match scope {
        "user" => Ok(paths::claude_user_dir().join("settings.json")),
        "user-local" => Ok(paths::claude_user_dir().join("settings.local.json")),
        "project" => {
            let base = project_path
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            Ok(base.join(".claude").join("settings.json"))
        }
        "project-local" => {
            let base = project_path
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            Ok(base.join(".claude").join("settings.local.json"))
        }
        _ => bail!("Unknown settings scope: {scope}"),
    }
}

pub fn load_settings(scope: &str, project_path: Option<&str>) -> Result<Value> {
    let path = settings_path(scope, project_path)?;
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(&path)?;
    let value: Value = serde_json::from_str(&content)?;
    Ok(value)
}

pub fn save_settings(scope: &str, project_path: Option<&str>, value: &Value) -> Result<()> {
    let path = settings_path(scope, project_path)?;
    let content = serde_json::to_string_pretty(value)?;
    paths::atomic_write(&path, &content)?;
    Ok(())
}

pub fn list_plugins() -> Result<Vec<PluginInfo>> {
    let cache_dir = paths::claude_user_dir().join("plugins").join("cache");
    if !cache_dir.exists() {
        return Ok(Vec::new());
    }

    let mut plugins = Vec::new();
    for entry in fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("plugin.json");
        if !manifest.exists() {
            continue;
        }
        let content = fs::read_to_string(&manifest)?;
        let value: Value = serde_json::from_str(&content)?;
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
            })
            .to_string();
        let description = value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let version = value
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        plugins.push(PluginInfo {
            name,
            description,
            version,
            dir_name,
        });
    }
    Ok(plugins)
}

pub fn list_skills() -> Result<Vec<SkillInfo>> {
    let skills_dir = paths::claude_user_dir().join("skills");
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut skills = Vec::new();
    for entry in fs::read_dir(&skills_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if !skill_md.exists() {
            continue;
        }
        let dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let content = fs::read_to_string(&skill_md).unwrap_or_default();
        let description = content.lines().take(3).collect::<Vec<_>>().join(" ");

        skills.push(SkillInfo {
            name: dir_name.clone(),
            dir_name,
            description,
        });
    }
    Ok(skills)
}

pub fn list_hooks_scripts() -> Result<Vec<HookScriptInfo>> {
    let hooks_dir = paths::claude_user_dir().join("hooks");
    if !hooks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut scripts = Vec::new();
    for entry in fs::read_dir(&hooks_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let full_path = path.to_string_lossy().to_string();
            scripts.push(HookScriptInfo {
                name,
                path: full_path,
            });
        }
    }
    Ok(scripts)
}

/// Before the `workbench` Claude Code plugin, Workbench reported activity
/// through a `workbench-hook-bridge` script registered in each account's
/// settings.json (a `.py`, then `.sh`/`.ps1`, in various quotings).
fn is_workbench_hook_command(command: &str) -> bool {
    command
        .to_ascii_lowercase()
        .replace('\\', "/")
        .contains("workbench-hook-bridge")
}

fn command_is_workbench_hook(value: &Value) -> bool {
    value
        .get("command")
        .and_then(|v| v.as_str())
        .is_some_and(is_workbench_hook_command)
}

/// Drops every Workbench bridge command, the entries that leaves empty and the
/// events that leaves empty. Everything else is kept as it was.
fn remove_workbench_hooks(hooks_obj: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    hooks_obj.retain(|_event, entries| {
        let Some(arr) = entries.as_array_mut() else {
            return true;
        };
        let before = arr.len();
        let mut emptied = false;
        for entry in arr.iter_mut() {
            if let Some(hooks) = entry.get_mut("hooks").and_then(|v| v.as_array_mut()) {
                let n = hooks.len();
                hooks.retain(|hook| !command_is_workbench_hook(hook));
                emptied |= n > 0 && hooks.is_empty();
                changed |= hooks.len() != n;
            }
        }
        if emptied || arr.iter().any(command_is_workbench_hook) {
            arr.retain(|entry| {
                !command_is_workbench_hook(entry)
                    && entry
                        .get("hooks")
                        .and_then(|v| v.as_array())
                        .is_none_or(|hooks| !hooks.is_empty())
            });
        }
        changed |= arr.len() != before;
        !(before > 0 && arr.is_empty())
    });
    changed
}

/// Removes the hook script and its registrations from every Claude account.
/// A settings file is rewritten only if it held one of Workbench's entries.
/// One account's failure doesn't stop the others; the first error is returned.
pub fn remove_workbench_hook_integration() -> Result<()> {
    let mut first_error = None;
    for (_, dir) in crate::claude_accounts::saved_config_dirs() {
        if let Err(e) = remove_hook_integration_in(&dir) {
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Settings first, scripts last: a script whose registration is still in a
/// settings file Workbench couldn't parse or rewrite stays, or every hook event
/// would fail on a missing command.
fn remove_hook_integration_in(claude_dir: &Path) -> Result<()> {
    let settings_path = claude_dir.join("settings.json");
    if let Ok(raw) = fs::read_to_string(&settings_path) {
        let mut settings: Value = serde_json::from_str(&raw)?;
        if let Some(hooks) = settings.get_mut("hooks").and_then(|v| v.as_object_mut()) {
            if remove_workbench_hooks(hooks) {
                if hooks.is_empty() {
                    if let Some(root) = settings.as_object_mut() {
                        root.remove("hooks");
                    }
                }
                paths::atomic_write(&settings_path, &serde_json::to_string_pretty(&settings)?)?;
            }
        }
    }
    for ext in ["sh", "ps1", "py"] {
        let _ = fs::remove_file(
            claude_dir
                .join("hooks")
                .join(format!("workbench-hook-bridge.{ext}")),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- settings_path ---

    #[test]
    fn settings_path_user_scope() {
        let path = settings_path("user", None).unwrap();
        assert!(path.ends_with("settings.json"));
        assert!(path.to_string_lossy().contains(".claude"));
    }

    #[test]
    fn settings_path_user_local_scope() {
        let path = settings_path("user-local", None).unwrap();
        assert!(path.ends_with("settings.local.json"));
        assert!(path.to_string_lossy().contains(".claude"));
    }

    #[test]
    fn settings_path_project_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let path = settings_path("project", Some(project)).unwrap();
        assert_eq!(path, dir.path().join(".claude").join("settings.json"));
    }

    #[test]
    fn settings_path_project_local_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        let path = settings_path("project-local", Some(project)).unwrap();
        assert_eq!(path, dir.path().join(".claude").join("settings.local.json"));
    }

    #[test]
    fn settings_path_project_without_path() {
        let path = settings_path("project", None).unwrap();
        assert_eq!(path, PathBuf::from("./.claude/settings.json"));
    }

    #[test]
    fn settings_path_unknown_scope_errors() {
        let result = settings_path("invalid", None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown settings scope"));
    }

    // --- load_settings / save_settings round-trip ---

    #[test]
    fn settings_round_trip_with_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_str().unwrap();
        let value = serde_json::json!({"foo": "bar", "nested": {"key": 42}});

        save_settings("project", Some(project_path), &value).unwrap();
        let loaded = load_settings("project", Some(project_path)).unwrap();
        assert_eq!(loaded, value);
    }

    #[test]
    fn load_settings_returns_empty_object_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_str().unwrap();

        let loaded = load_settings("project", Some(project_path)).unwrap();
        assert_eq!(loaded, serde_json::json!({}));
    }

    #[test]
    fn save_settings_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let project_path = dir.path().to_str().unwrap();

        let v1 = serde_json::json!({"version": 1});
        let v2 = serde_json::json!({"version": 2, "extra": true});

        save_settings("project", Some(project_path), &v1).unwrap();
        save_settings("project", Some(project_path), &v2).unwrap();
        let loaded = load_settings("project", Some(project_path)).unwrap();
        assert_eq!(loaded, v2);
    }

    // --- remove_workbench_hooks ---

    fn commands(hooks_obj: &serde_json::Map<String, Value>, event: &str) -> Vec<String> {
        hooks_obj[event]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(
                |entry| match entry.get("hooks").and_then(|v| v.as_array()) {
                    Some(hooks) => hooks.iter().map(|h| h["command"].clone()).collect(),
                    None => vec![entry["command"].clone()],
                },
            )
            .map(|c| c.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn removes_every_form_of_the_bridge_and_keeps_other_hooks() {
        let mut hooks_obj = serde_json::json!({
            "SessionStart": [
                { "command": "python ~/.claude/hooks/workbench-hook-bridge.py" },
                { "hooks": [{ "type": "command", "command": "/home/me/.claude/hooks/workbench-hook-bridge.sh" }] },
                { "hooks": [{ "type": "command", "command": "/usr/local/bin/other-hook" }] }
            ],
            "PostToolUse": [
                { "matcher": "Bash", "hooks": [
                    { "type": "command", "command": "pwsh -File 'C:/x/workbench-hook-bridge.ps1'" },
                    { "type": "command", "command": "/usr/local/bin/lint" }
                ] }
            ],
            "Stop": [
                { "hooks": [{ "type": "command", "command": "pwsh -File C:\\x\\Workbench-Hook-Bridge.ps1" }] }
            ]
        })
        .as_object()
        .unwrap()
        .clone();

        assert!(remove_workbench_hooks(&mut hooks_obj));

        assert_eq!(
            commands(&hooks_obj, "SessionStart"),
            ["/usr/local/bin/other-hook"]
        );
        assert_eq!(commands(&hooks_obj, "PostToolUse"), ["/usr/local/bin/lint"]);
        assert_eq!(
            hooks_obj["PostToolUse"][0]["matcher"], "Bash",
            "an entry keeping other hooks keeps its matcher"
        );
        assert!(
            !hooks_obj.contains_key("Stop"),
            "an event left empty is dropped"
        );
    }

    #[test]
    fn leaves_settings_without_the_bridge_untouched() {
        let original = serde_json::json!({
            "Stop": [{ "hooks": [{ "type": "command", "command": "/usr/local/bin/other-hook" }] }],
            "Notification": []
        });
        let mut hooks_obj = original.as_object().unwrap().clone();
        assert!(!remove_workbench_hooks(&mut hooks_obj));
        assert_eq!(Value::Object(hooks_obj), original);
    }

    #[test]
    fn cleans_an_account_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();
        let script = hooks_dir.join("workbench-hook-bridge.sh");
        fs::write(&script, "#!/bin/sh").unwrap();
        let settings_path = dir.path().join("settings.json");
        let settings = serde_json::json!({
            "model": "opus",
            "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": script.to_string_lossy() }] }] }
        });
        fs::write(&settings_path, settings.to_string()).unwrap();

        remove_hook_integration_in(dir.path()).unwrap();

        assert!(!script.exists());
        let saved: Value =
            serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({ "model": "opus" }));
    }

    #[test]
    fn keeps_the_script_when_settings_cannot_be_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("hooks/workbench-hook-bridge.sh");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, "#!/bin/sh").unwrap();
        fs::write(dir.path().join("settings.json"), "{ not json").unwrap();

        assert!(remove_hook_integration_in(dir.path()).is_err());
        assert!(script.exists());
    }

    #[test]
    fn cleaning_never_creates_a_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        remove_hook_integration_in(dir.path()).unwrap();
        assert!(!dir.path().join("settings.json").exists());
    }
}
