use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::io::BufRead;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::types::{DiscoveredClaudeSession, ProjectConfig, ProjectsFile, WorkspaceFile};

const WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME: &str = "workbench-codex-notify-bridge.py";

fn config_path() -> PathBuf {
    paths::workbench_config_dir().join("projects.json")
}

pub fn load_projects() -> Result<Vec<ProjectConfig>> {
    let path = config_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path)?;
    let file: ProjectsFile = serde_json::from_str(&content)?;
    Ok(file.projects)
}

pub fn save_projects(projects: &[ProjectConfig]) -> Result<()> {
    let file = ProjectsFile {
        projects: projects.to_vec(),
    };
    let content = serde_json::to_string_pretty(&file)?;
    paths::atomic_write(&config_path(), &content)?;
    Ok(())
}

fn workspace_path() -> PathBuf {
    paths::workbench_config_dir().join("workspaces.json")
}

pub fn load_workspaces() -> Result<WorkspaceFile> {
    let path = workspace_path();
    if !path.exists() {
        return Ok(WorkspaceFile {
            workspaces: Vec::new(),
            selected_id: None,
        });
    }

    let content = fs::read_to_string(&path)?;
    let file: WorkspaceFile = serde_json::from_str(&content)?;
    Ok(file)
}

pub fn save_workspaces(file: &WorkspaceFile) -> Result<()> {
    let content = serde_json::to_string_pretty(file)?;
    paths::atomic_write(&workspace_path(), &content)?;
    Ok(())
}

/// Encode a project path the same way Claude CLI does: replace `/` with `-`
fn encode_project_path(project_path: &str) -> String {
    project_path.replace('/', "-")
}

/// Parse a single JSONL session file into a DiscoveredClaudeSession.
fn parse_session_jsonl(
    path: &std::path::Path,
    session_id: String,
) -> Option<DiscoveredClaudeSession> {
    let file = fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);

    let mut label = String::new();
    let mut timestamp = String::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let obj: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Grab timestamp from first entry
        if timestamp.is_empty() {
            if let Some(ts) = obj.get("timestamp").and_then(|v| v.as_str()) {
                timestamp = ts.to_string();
            }
        }

        // Find first real user message text
        if obj.get("type").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        // Skip meta/system messages
        if obj.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }

        let content = obj.get("message").and_then(|m| m.get("content"));

        let text = match content {
            // content can be a plain string or an array of content blocks
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Array(arr)) => arr.iter().find_map(|item| {
                if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                    item.get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            }),
            _ => None,
        };

        if let Some(raw) = text {
            let trimmed = raw.trim();
            // Skip system/tool/command messages
            if trimmed.is_empty()
                || trimmed.len() <= 5
                || trimmed.starts_with('<')
                || trimmed.starts_with("[Request interrupted")
                || trimmed.starts_with("Base directory")
            {
                continue;
            }
            // Truncate to first line or 80 chars
            let first_line = trimmed.lines().next().unwrap_or(trimmed);
            label = if first_line.len() > 80 {
                format!("{}...", &first_line[..77])
            } else {
                first_line.to_string()
            };
            break;
        }
    }

    if label.is_empty() {
        label = format!("Session {}", &session_id[..8.min(session_id.len())]);
    }

    Some(DiscoveredClaudeSession {
        session_id,
        label,
        timestamp,
        last_message_role: None,
    })
}

/// Discover Claude CLI sessions by reading ~/.claude/projects/<encoded-path>/*.jsonl
pub fn discover_claude_sessions(project_path: &str) -> Result<Vec<DiscoveredClaudeSession>> {
    let encoded = encode_project_path(project_path);
    let sessions_dir = paths::claude_user_dir().join("projects").join(&encoded);

    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if session_id.is_empty() {
            continue;
        }

        if let Some(session) = parse_session_jsonl(&path, session_id) {
            sessions.push(session);
        }
    }

    // Sort by timestamp descending (newest first)
    sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Ok(sessions)
}

fn codex_session_meta_candidates<'a>(obj: &'a serde_json::Value) -> [Option<&'a serde_json::Value>; 4] {
    [
        obj.pointer("/item/meta"),
        obj.get("meta"),
        obj.pointer("/item/payload"),
        obj.get("payload"),
    ]
}

fn codex_cwd_matches_project(cwd: &str, project_path: &Path) -> bool {
    let cwd_canon = fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from(cwd));
    cwd_canon == *project_path
}

fn is_codex_session_meta_row(obj: &serde_json::Value, line_index: usize) -> bool {
    if line_index == 0 {
        return true;
    }
    if obj.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
        return true;
    }
    obj.pointer("/item/type").and_then(|v| v.as_str()) == Some("session_meta")
}

fn parse_codex_session_jsonl(path: &Path, project_path: &Path) -> Option<DiscoveredClaudeSession> {
    let file = fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);

    let mut session_id = String::new();
    let mut timestamp = String::new();
    let mut cwd_matches = false;
    let mut label = String::new();

    for (i, line) in reader.lines().enumerate() {
        if i > 200 {
            break;
        }
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let obj: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if is_codex_session_meta_row(&obj, i) {
            // Capture session metadata; current Codex files place it in payload on session_meta rows.
            for meta in codex_session_meta_candidates(&obj).into_iter().flatten() {
                if let Some(cwd) = meta.get("cwd").and_then(|v| v.as_str()) {
                    if !codex_cwd_matches_project(cwd, project_path) {
                        return None;
                    }
                    cwd_matches = true;
                }
                if session_id.is_empty() {
                    if let Some(id) = meta.get("id").and_then(|v| v.as_str()) {
                        session_id = id.to_string();
                    }
                }
                if timestamp.is_empty() {
                    if let Some(ts) = meta.get("timestamp").and_then(|v| v.as_str()) {
                        timestamp = ts.to_string();
                    } else if let Some(ts) = meta.get("timestamp").and_then(|v| v.as_f64()) {
                        timestamp = ts.to_string();
                    }
                }
            }

            // Fallback to top-level metadata fields.
            if session_id.is_empty() {
                if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                    session_id = id.to_string();
                }
            }
            if timestamp.is_empty() {
                if let Some(ts) = obj.get("timestamp").and_then(|v| v.as_str()) {
                    timestamp = ts.to_string();
                }
            }
            if !cwd_matches {
                if let Some(cwd) = obj.get("cwd").and_then(|v| v.as_str()) {
                    if !codex_cwd_matches_project(cwd, project_path) {
                        return None;
                    }
                    cwd_matches = true;
                }
            }
        }

        // Skip the first row after harvesting metadata.
        if i == 0 {
            continue;
        }

        // Look for first user message (EventMsg with user content)
        // Codex format: look for user messages in various possible shapes
        let text = extract_codex_user_message(&obj);
        if let Some(raw) = text {
            let mut trimmed = raw.trim().to_string();
            // Strip Codex's request prefix
            if let Some(rest) = trimmed.strip_prefix("## My request for Codex:\n") {
                trimmed = rest.trim().to_string();
            }
            if trimmed.is_empty() || trimmed.len() <= 5 {
                continue;
            }
            let first_line = trimmed.lines().next().unwrap_or(&trimmed);
            label = if first_line.len() > 80 {
                format!("{}...", &first_line[..77])
            } else {
                first_line.to_string()
            };
            break;
        }
    }

    if !cwd_matches {
        return None;
    }

    // Fall back to extracting session ID from filename if not found in content
    if session_id.is_empty() {
        // Filename format: rollout-<ts>-<uuid>.jsonl
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        session_id = stem.to_string();
    }

    if session_id.is_empty() {
        return None;
    }

    if label.is_empty() {
        label = format!("Session {}", &session_id[..8.min(session_id.len())]);
    }

    Some(DiscoveredClaudeSession {
        session_id,
        label,
        timestamp,
        last_message_role: None,
    })
}

/// Extract user message text from a Codex JSONL event object.
fn extract_codex_user_message(obj: &serde_json::Value) -> Option<String> {
    // Try: { "type": "user", "message": { "content": ... } } (Claude-like)
    if obj.get("type").and_then(|v| v.as_str()) == Some("user") {
        let content = obj.get("message").and_then(|m| m.get("content"));
        return extract_text_from_content(content);
    }

    // Try: { "item": { "role": "user", "content": [...] } }
    if let Some(item) = obj.get("item") {
        if item.get("role").and_then(|v| v.as_str()) == Some("user") {
            let content = item.get("content");
            return extract_text_from_content(content);
        }
    }

    // Try: { "role": "user", "content": ... }
    if obj.get("role").and_then(|v| v.as_str()) == Some("user") {
        let content = obj.get("content");
        return extract_text_from_content(content);
    }

    None
}

/// Extract text from a content field (string or array of content blocks).
fn extract_text_from_content(content: Option<&serde_json::Value>) -> Option<String> {
    match content {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(arr)) => arr.iter().find_map(|item| {
            if item.get("type").and_then(|v| v.as_str()) == Some("text")
                || item.get("type").and_then(|v| v.as_str()) == Some("input_text")
            {
                item.get("text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }),
        _ => None,
    }
}

/// Recursively walk a directory up to `max_depth` levels and collect .jsonl files.
fn collect_jsonl_files(dir: &Path, max_depth: u32) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if max_depth == 0 || !dir.is_dir() {
        return results;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            results.extend(collect_jsonl_files(&path, max_depth - 1));
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            results.push(path);
        }
    }
    results
}

/// Discover Codex CLI sessions by reading ~/.codex/sessions/**/*.jsonl
/// and filtering by cwd match against the given project_path.
pub fn discover_codex_sessions(project_path: &str) -> Result<Vec<DiscoveredClaudeSession>> {
    let sessions_dir = paths::codex_sessions_dir();
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let canonical_project =
        fs::canonicalize(project_path).unwrap_or_else(|_| PathBuf::from(project_path));

    let jsonl_files = collect_jsonl_files(&sessions_dir, 4); // year/month/day/file

    let mut sessions = Vec::new();
    for path in jsonl_files {
        if let Some(session) = parse_codex_session_jsonl(&path, &canonical_project) {
            sessions.push(session);
        }
    }

    sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(sessions)
}

fn workbench_codex_notify_script_path() -> PathBuf {
    paths::codex_config_dir().join(WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME)
}

fn workbench_codex_notify_script_body() -> &'static str {
    r#"#!/usr/bin/env python3
import json
import os
import socket
import sys

socket_path = os.environ.get("WORKBENCH_HOOK_SOCKET")
pane_id = os.environ.get("WORKBENCH_PANE_ID")
if not socket_path or not pane_id or len(sys.argv) < 2:
    sys.exit(0)

try:
    payload = json.loads(sys.argv[1])
except Exception:
    sys.exit(0)

envelope = {"pane_id": pane_id, "codex": payload}

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

fn ensure_workbench_codex_notify_script() -> Result<PathBuf> {
    let codex_dir = paths::codex_config_dir();
    fs::create_dir_all(&codex_dir)?;

    let script_path = workbench_codex_notify_script_path();
    let desired = workbench_codex_notify_script_body();
    let current = fs::read_to_string(&script_path).unwrap_or_default();
    if current != desired {
        fs::write(&script_path, desired)?;
    }

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms)?;
    }

    Ok(script_path)
}

fn toml_escape_str(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path, visited: &mut HashSet<PathBuf>) -> Result<()> {
    let canonical_src = fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    if !visited.insert(canonical_src.clone()) {
        return Ok(());
    }

    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let meta = fs::symlink_metadata(&src_path)?;

        if meta.file_type().is_symlink() {
            let resolved = match fs::canonicalize(&src_path) {
                Ok(path) => path,
                Err(_) => continue,
            };
            let resolved_meta = match fs::metadata(&resolved) {
                Ok(meta) => meta,
                Err(_) => continue,
            };

            if resolved_meta.is_dir() {
                copy_dir_recursive(&resolved, &dst_path, visited)?;
            } else if resolved_meta.is_file() {
                if let Some(parent) = dst_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&resolved, &dst_path)?;
            }
            continue;
        }

        if meta.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, visited)?;
        } else if meta.is_file() {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src_path, &dst_path)?;
        }
    }

    visited.remove(&canonical_src);
    Ok(())
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
        copy_dir_recursive(&destination_skills, &staging_skills, &mut visited)?;
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
        copy_dir_recursive(&resolved, &dst, &mut visited)?;
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

fn ensure_codex_notify_config(content: &str, script_path: &str) -> (String, bool) {
    let escaped_path = toml_escape_str(script_path);
    let notify_line = format!("notify = [\"python3\", \"{}\"]", escaped_path);

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
    let (with_notify, notify_changed) = ensure_codex_notify_config(&updated_content, &script_path_str);
    if notify_changed {
        updated_content = with_notify;
        changed = true;
    }

    if changed {
        fs::write(&config_path, updated_content)?;
    }

    // Ensure Codex can discover user skills from Claude skill directories.
    sync_claude_skills_into_agents()?;

    Ok(())
}
