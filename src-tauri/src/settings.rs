use anyhow::{bail, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::types::{HookScriptInfo, PluginInfo, SkillInfo};

const WORKBENCH_HOOK_SCRIPT_NAME: &str = "workbench-hook-bridge.py";
const WORKBENCH_HOOK_EVENTS: [&str; 4] =
    ["SessionStart", "UserPromptSubmit", "Stop", "Notification"];

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

fn workbench_hook_script_path() -> PathBuf {
    paths::claude_user_dir()
        .join("hooks")
        .join(WORKBENCH_HOOK_SCRIPT_NAME)
}

fn workbench_hook_script_body() -> &'static str {
    r#"#!/usr/bin/env python3
import json
import os
import platform
import socket
import sys

socket_path = os.environ.get("WORKBENCH_HOOK_SOCKET")
pane_id = os.environ.get("WORKBENCH_PANE_ID")
if not socket_path or not pane_id:
    sys.exit(0)

raw = sys.stdin.read()
if not raw or not raw.strip():
    sys.exit(0)

try:
    hook_payload = json.loads(raw)
except Exception:
    sys.exit(0)

envelope = {"pane_id": pane_id, "hook": hook_payload}

try:
    if ":" in socket_path and not socket_path.startswith("/"):
        host, port = socket_path.rsplit(":", 1)
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.connect((host, int(port)))
    else:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(socket_path)
    msg = json.dumps(envelope, separators=(",", ":")) + "\n"
    sock.sendall(msg.encode("utf-8"))
    sock.close()
except Exception:
    pass
"#
}

fn ensure_workbench_hook_script() -> Result<PathBuf> {
    paths::ensure_script(
        &workbench_hook_script_path(),
        workbench_hook_script_body(),
    )
}

fn ensure_object(value: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(serde_json::Map::new());
    }
    value.as_object_mut().expect("object just initialized")
}

fn ensure_event_hooks(
    hooks_obj: &mut serde_json::Map<String, Value>,
    event_name: &str,
    command: &str,
) -> bool {
    let mut changed = false;

    let event_value = hooks_obj
        .entry(event_name.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !event_value.is_array() {
        *event_value = Value::Array(Vec::new());
        changed = true;
    }
    let entries = event_value
        .as_array_mut()
        .expect("event value should be normalized to array");

    let already_present = entries.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|v| v.as_array())
            .is_some_and(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("type").and_then(|v| v.as_str()) == Some("command")
                        && hook.get("command").and_then(|v| v.as_str()) == Some(command)
                })
            })
    });

    if !already_present {
        entries.push(serde_json::json!({
            "hooks": [
                {
                    "type": "command",
                    "command": command
                }
            ]
        }));
        changed = true;
    }

    changed
}

/// Build the hook command string for a given script path.
/// On Windows, prefix with `python3` since shebangs don't work.
fn hook_command_for_script(script_path: &Path) -> String {
    if cfg!(windows) {
        format!("python3 {}", script_path.to_string_lossy())
    } else {
        script_path.to_string_lossy().to_string()
    }
}

pub fn check_workbench_hook_integration() -> crate::types::IntegrationStatus {
    let script_path = workbench_hook_script_path();
    let script_exists = script_path.exists();

    let settings_path = paths::claude_user_dir().join("settings.json");
    let settings = if settings_path.exists() {
        fs::read_to_string(&settings_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let command = hook_command_for_script(&script_path);
    let mut missing_events = Vec::new();

    for event in WORKBENCH_HOOK_EVENTS {
        let already_present = settings
            .pointer(&format!("/hooks/{}", event))
            .and_then(|v| v.as_array())
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry
                        .get("hooks")
                        .and_then(|v| v.as_array())
                        .is_some_and(|hooks| {
                            hooks.iter().any(|hook| {
                                hook.get("type").and_then(|v| v.as_str()) == Some("command")
                                    && hook.get("command").and_then(|v| v.as_str())
                                        == Some(&command)
                            })
                        })
                })
            });
        if !already_present {
            missing_events.push(event);
        }
    }

    let needs_changes = !script_exists || !missing_events.is_empty();
    let description = if needs_changes {
        "Workbench will install a hook script and register it in your Claude Code settings (~/.claude/settings.json) for the following events: SessionStart, UserPromptSubmit, Stop, Notification. This enables session activity tracking.".to_string()
    } else {
        String::new()
    };

    crate::types::IntegrationStatus {
        needs_changes,
        description,
    }
}

pub fn ensure_workbench_hook_integration() -> Result<()> {
    let script_path = ensure_workbench_hook_script()?;
    let settings_path = paths::claude_user_dir().join("settings.json");
    let mut settings = if settings_path.exists() {
        let raw = fs::read_to_string(&settings_path)?;
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let root = ensure_object(&mut settings);
    let hooks_value = root
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let hooks_obj = ensure_object(hooks_value);

    let command = hook_command_for_script(&script_path);
    let mut changed = false;
    for event in WORKBENCH_HOOK_EVENTS {
        changed |= ensure_event_hooks(hooks_obj, event, &command);
    }

    if changed || !settings_path.exists() {
        let content = serde_json::to_string_pretty(&settings)?;
        paths::atomic_write(&settings_path, &content)?;
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
        assert_eq!(
            path,
            dir.path().join(".claude").join("settings.local.json")
        );
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown settings scope")
        );
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

    // --- hook_command_for_script ---

    #[test]
    fn hook_command_for_script_returns_nonempty() {
        let path = PathBuf::from("/some/script.py");
        let cmd = hook_command_for_script(&path);
        assert!(!cmd.is_empty());
        assert!(cmd.contains("script.py"));
    }

    #[cfg(windows)]
    #[test]
    fn hook_command_for_script_windows_prefixes_python() {
        let path = PathBuf::from("C:\\Users\\test\\.claude\\hooks\\workbench-hook-bridge.py");
        let cmd = hook_command_for_script(&path);
        assert!(cmd.starts_with("python3 "));
        assert!(cmd.contains("workbench-hook-bridge.py"));
    }

    #[cfg(unix)]
    #[test]
    fn hook_command_for_script_unix_is_bare_path() {
        let path = PathBuf::from("/home/user/.claude/hooks/workbench-hook-bridge.py");
        let cmd = hook_command_for_script(&path);
        assert_eq!(cmd, "/home/user/.claude/hooks/workbench-hook-bridge.py");
        assert!(!cmd.starts_with("python3"));
    }
}
