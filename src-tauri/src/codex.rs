/// Codex CLI session discovery and configuration management.
use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::paths::{copy_dir_follow_symlinks, remove_path_if_exists};

use crate::paths;
use crate::session_utils;
use crate::types::DiscoveredClaudeSession;

const WORKBENCH_CODEX_NOTIFY_SCRIPT_NAME: &str = "workbench-codex-notify-bridge.py";
const CODEX_JSONL_MAX_SCAN_LINES: usize = 200;

// ---------------------------------------------------------------------------
// Session discovery
// ---------------------------------------------------------------------------

fn codex_session_meta_candidates<'a>(
    obj: &'a serde_json::Value,
) -> [Option<&'a serde_json::Value>; 4] {
    [
        obj.pointer("/item/meta"),
        obj.get("meta"),
        obj.pointer("/item/payload"),
        obj.get("payload"),
    ]
}

fn strip_codex_request_prefix(raw: &str) -> String {
    let trimmed = raw.trim();
    for prefix in [
        "## My request for Codex:\r\n",
        "## My request for Codex:\n",
        "## My request for Codex:",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn is_codex_bootstrap_message(text: &str) -> bool {
    let first_line = text.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return true;
    }

    first_line.starts_with("# AGENTS.md instructions for ")
        || first_line.starts_with("# AGENTS")
        || first_line.starts_with("# CLAUDE.md")
        || first_line.starts_with("<environment_context>")
        || first_line.starts_with("<permissions instructions>")
        || first_line.starts_with("<app-context>")
        || first_line.starts_with("<collaboration_mode>")
        || first_line.starts_with("<INSTRUCTIONS>")
        || first_line.starts_with("Warning: apply_patch was requested via exec_command.")
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
        if i > CODEX_JSONL_MAX_SCAN_LINES {
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

        let text = extract_codex_user_message(&obj);
        if let Some(raw) = text {
            let trimmed = strip_codex_request_prefix(&raw);
            if trimmed.len() <= 5 || is_codex_bootstrap_message(&trimmed) {
                continue;
            }
            label = session_utils::truncate_label(&trimmed);
            break;
        }
    }

    if !cwd_matches {
        return None;
    }

    // Fall back to extracting session ID from filename if not found in content
    if session_id.is_empty() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        session_id = stem.to_string();
    }

    if session_id.is_empty() {
        return None;
    }

    if label.is_empty() {
        label = session_utils::fallback_label(&session_id);
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
    // Try: { "type": "response_item", "payload": { "role": "user", "type": "message", "content": [...] } }
    if obj.get("type").and_then(|v| v.as_str()) == Some("response_item") {
        if let Some(payload) = obj.get("payload") {
            let is_user_message = payload.get("role").and_then(|v| v.as_str()) == Some("user")
                && payload.get("type").and_then(|v| v.as_str()) == Some("message");
            if is_user_message {
                return session_utils::extract_text_from_content(payload.get("content"));
            }
        }
    }

    // Try: { "type": "user", "message": { "content": ... } } (Claude-like)
    if obj.get("type").and_then(|v| v.as_str()) == Some("user") {
        let content = obj.get("message").and_then(|m| m.get("content"));
        return session_utils::extract_text_from_content(content);
    }

    // Try: { "item": { "role": "user", "content": [...] } }
    if let Some(item) = obj.get("item") {
        if item.get("role").and_then(|v| v.as_str()) == Some("user") {
            let content = item.get("content");
            return session_utils::extract_text_from_content(content);
        }
    }

    // Try: { "role": "user", "content": ... }
    if obj.get("role").and_then(|v| v.as_str()) == Some("user") {
        let content = obj.get("content");
        return session_utils::extract_text_from_content(content);
    }

    // Try: { "type": "event_msg", "payload": { "type": "user_message", "text": ... } }
    if obj.get("type").and_then(|v| v.as_str()) == Some("event_msg")
        && obj.pointer("/payload/type").and_then(|v| v.as_str()) == Some("user_message")
    {
        if let Some(text) = obj.pointer("/payload/text").and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
    }

    None
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

// ---------------------------------------------------------------------------
// Codex configuration management
// ---------------------------------------------------------------------------

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
    let (with_notify, notify_changed) =
        ensure_codex_notify_config(&updated_content, &script_path_str);
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
