use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::paths;
use crate::types::{HookScriptInfo, PluginInfo, SkillInfo};

fn settings_path(scope: &str, project_path: Option<&str>) -> PathBuf {
    match scope {
        "user" => paths::claude_user_dir().join("settings.json"),
        "user-local" => paths::claude_user_dir().join("settings.local.json"),
        "project" => {
            let base = project_path
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            base.join(".claude").join("settings.json")
        }
        "project-local" => {
            let base = project_path
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            base.join(".claude").join("settings.local.json")
        }
        _ => paths::claude_user_dir().join("settings.json"),
    }
}

pub fn load_settings(scope: &str, project_path: Option<&str>) -> Result<Value> {
    let path = settings_path(scope, project_path);
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(&path)?;
    let value: Value = serde_json::from_str(&content)?;
    Ok(value)
}

pub fn save_settings(scope: &str, project_path: Option<&str>, value: &Value) -> Result<()> {
    let path = settings_path(scope, project_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(value)?;
    fs::write(&path, content)?;
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
