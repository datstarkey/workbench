/// Claude CLI session discovery — reads ~/.claude/projects/<encoded-path>/*.jsonl
use anyhow::Result;
use std::fs;
use std::io::BufRead;

use crate::paths;
use crate::session_utils;
use crate::types::DiscoveredClaudeSession;

/// Parse a single JSONL session file into a DiscoveredClaudeSession.
pub(crate) fn parse_session_jsonl(
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
    let encoded = paths::encode_project_path(project_path);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    // parse_session_jsonl tests

    #[test]
    fn parse_valid_session_with_user_message() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("abc12345-1234-1234-1234-123456789abc.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:00:00Z","message":{{"content":"Fix the login bug"}}}}"#
        )
        .unwrap();

        let result =
            parse_session_jsonl(&path, "abc12345-1234-1234-1234-123456789abc".to_string());
        assert!(result.is_some());
        let session = result.unwrap();
        assert_eq!(session.session_id, "abc12345-1234-1234-1234-123456789abc");
        assert_eq!(session.label, "Fix the login bug");
        assert_eq!(session.timestamp, "2024-01-15T10:00:00Z");
    }

    #[test]
    fn parse_session_with_array_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:00:00Z","message":{{"content":[{{"type":"text","text":"Refactor the auth module"}}]}}}}"#
        )
        .unwrap();

        let result = parse_session_jsonl(&path, "session123".to_string());
        let session = result.unwrap();
        assert_eq!(session.label, "Refactor the auth module");
    }

    #[test]
    fn parse_session_only_meta_messages_falls_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:00:00Z","isMeta":true,"message":{{"content":"meta info"}}}}"#
        )
        .unwrap();

        let result = parse_session_jsonl(&path, "abcdef1234567890".to_string());
        let session = result.unwrap();
        assert_eq!(session.label, "Session abcdef12");
        assert_eq!(session.timestamp, "2024-01-15T10:00:00Z");
    }

    #[test]
    fn parse_session_skips_meta_finds_real_message() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:00:00Z","isMeta":true,"message":{{"content":"meta info"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:01:00Z","message":{{"content":"Real user message here"}}}}"#
        )
        .unwrap();

        let result = parse_session_jsonl(&path, "abcdef1234567890".to_string());
        let session = result.unwrap();
        assert_eq!(session.label, "Real user message here");
        // Timestamp should come from the first entry
        assert_eq!(session.timestamp, "2024-01-15T10:00:00Z");
    }

    #[test]
    fn parse_session_empty_file_returns_fallback() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        fs::File::create(&path).unwrap();

        let result = parse_session_jsonl(&path, "abcdef1234567890".to_string());
        let session = result.unwrap();
        assert_eq!(session.label, "Session abcdef12");
        assert_eq!(session.timestamp, "");
    }

    #[test]
    fn parse_session_skips_assistant_messages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"assistant","timestamp":"2024-01-15T10:00:00Z","message":{{"content":"I can help with that"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:01:00Z","message":{{"content":"Add unit tests to the project"}}}}"#
        )
        .unwrap();

        let result = parse_session_jsonl(&path, "session456".to_string());
        let session = result.unwrap();
        assert_eq!(session.label, "Add unit tests to the project");
        assert_eq!(session.timestamp, "2024-01-15T10:00:00Z");
    }

    #[test]
    fn parse_session_skips_short_user_messages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:00:00Z","message":{{"content":"hi"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:01:00Z","message":{{"content":"Please fix the broken tests"}}}}"#
        )
        .unwrap();

        let result = parse_session_jsonl(&path, "session789".to_string());
        let session = result.unwrap();
        assert_eq!(session.label, "Please fix the broken tests");
    }

    #[test]
    fn parse_session_nonexistent_file_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.jsonl");

        let result = parse_session_jsonl(&path, "session000".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn parse_session_invalid_json_lines_skipped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "not valid json").unwrap();
        writeln!(
            file,
            r#"{{"type":"user","timestamp":"2024-01-15T10:00:00Z","message":{{"content":"Valid message after garbage"}}}}"#
        )
        .unwrap();

        let result = parse_session_jsonl(&path, "session111".to_string());
        let session = result.unwrap();
        assert_eq!(session.label, "Valid message after garbage");
        assert_eq!(session.timestamp, "2024-01-15T10:00:00Z");
    }
}
