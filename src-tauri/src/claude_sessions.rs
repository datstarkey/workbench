/// Claude CLI session discovery — reads ~/.claude/projects/<encoded-path>/*.jsonl
use anyhow::Result;
use std::fs;
use std::io::BufRead;

use crate::paths;
use crate::session_utils;
use crate::types::DiscoveredClaudeSession;

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
        let text = session_utils::extract_text_from_content(content);

        if let Some(raw) = text {
            let trimmed = raw.trim();
            if session_utils::is_skippable_user_message(trimmed) {
                continue;
            }
            label = session_utils::truncate_label(trimmed);
            break;
        }
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
