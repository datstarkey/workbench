use anyhow::{bail, Result};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::paths;
use crate::types::{HookScriptInfo, PluginInfo, SkillInfo};

const WORKBENCH_HOOK_SCRIPT_NAME: &str = "workbench-hook-bridge.py";
const WORKBENCH_HOOK_EVENTS: [&str; 4] =
    ["SessionStart", "UserPromptSubmit", "Stop", "Notification"];

fn settings_path(scope: &str, project_path: Option<&str>) -> Result<PathBuf> {
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

    let command = script_path.to_string_lossy().to_string();
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
