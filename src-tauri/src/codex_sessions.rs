/// Codex CLI session discovery.
use anyhow::Result;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::session_utils;
use crate::types::DiscoveredClaudeSession;

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

pub(crate) fn strip_codex_request_prefix(raw: &str) -> String {
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

pub(crate) fn is_codex_bootstrap_message(text: &str) -> bool {
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
pub(crate) fn extract_codex_user_message(obj: &serde_json::Value) -> Option<String> {
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
pub(crate) fn collect_jsonl_files(dir: &Path, max_depth: u32) -> Vec<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // strip_codex_request_prefix
    // -----------------------------------------------------------------------

    #[test]
    fn strip_prefix_with_newline() {
        assert_eq!(
            strip_codex_request_prefix("## My request for Codex:\nHello world"),
            "Hello world"
        );
    }

    #[test]
    fn strip_prefix_with_crlf() {
        assert_eq!(
            strip_codex_request_prefix("## My request for Codex:\r\nHello world"),
            "Hello world"
        );
    }

    #[test]
    fn strip_prefix_no_separator() {
        assert_eq!(
            strip_codex_request_prefix("## My request for Codex:Hello"),
            "Hello"
        );
    }

    #[test]
    fn strip_prefix_no_match() {
        assert_eq!(
            strip_codex_request_prefix("Just a normal message"),
            "Just a normal message"
        );
    }

    #[test]
    fn strip_prefix_trims_whitespace() {
        assert_eq!(
            strip_codex_request_prefix("  ## My request for Codex:\n  Hello  "),
            "Hello"
        );
    }

    // -----------------------------------------------------------------------
    // is_codex_bootstrap_message
    // -----------------------------------------------------------------------

    #[test]
    fn bootstrap_agents_md() {
        assert!(is_codex_bootstrap_message(
            "# AGENTS.md instructions for foo"
        ));
    }

    #[test]
    fn bootstrap_agents_short() {
        assert!(is_codex_bootstrap_message("# AGENTS"));
    }

    #[test]
    fn bootstrap_claude_md() {
        assert!(is_codex_bootstrap_message("# CLAUDE.md"));
    }

    #[test]
    fn bootstrap_environment_context() {
        assert!(is_codex_bootstrap_message("<environment_context>"));
    }

    #[test]
    fn bootstrap_permissions() {
        assert!(is_codex_bootstrap_message("<permissions instructions>"));
    }

    #[test]
    fn bootstrap_app_context() {
        assert!(is_codex_bootstrap_message("<app-context>"));
    }

    #[test]
    fn bootstrap_collaboration_mode() {
        assert!(is_codex_bootstrap_message("<collaboration_mode>"));
    }

    #[test]
    fn bootstrap_instructions() {
        assert!(is_codex_bootstrap_message("<INSTRUCTIONS>"));
    }

    #[test]
    fn bootstrap_apply_patch_warning() {
        assert!(is_codex_bootstrap_message(
            "Warning: apply_patch was requested via exec_command. Something else."
        ));
    }

    #[test]
    fn bootstrap_empty_string() {
        assert!(is_codex_bootstrap_message(""));
    }

    #[test]
    fn bootstrap_normal_message_is_false() {
        assert!(!is_codex_bootstrap_message("Normal user message"));
    }

    #[test]
    fn bootstrap_multiline_with_empty_first_line() {
        assert!(is_codex_bootstrap_message("\nSome content after"));
    }

    // -----------------------------------------------------------------------
    // extract_codex_user_message
    // -----------------------------------------------------------------------

    #[test]
    fn extract_response_item_format() {
        let obj = serde_json::json!({
            "type": "response_item",
            "payload": {
                "role": "user",
                "type": "message",
                "content": [{"type": "input_text", "text": "hello from response_item"}]
            }
        });
        assert_eq!(
            extract_codex_user_message(&obj),
            Some("hello from response_item".to_string())
        );
    }

    #[test]
    fn extract_claude_like_user_format() {
        let obj = serde_json::json!({
            "type": "user",
            "message": {
                "content": [{"type": "text", "text": "hello from claude format"}]
            }
        });
        assert_eq!(
            extract_codex_user_message(&obj),
            Some("hello from claude format".to_string())
        );
    }

    #[test]
    fn extract_item_role_user_format() {
        let obj = serde_json::json!({
            "item": {
                "role": "user",
                "content": [{"type": "input_text", "text": "hello from item"}]
            }
        });
        assert_eq!(
            extract_codex_user_message(&obj),
            Some("hello from item".to_string())
        );
    }

    #[test]
    fn extract_direct_role_user_format() {
        let obj = serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "hello direct"}]
        });
        assert_eq!(
            extract_codex_user_message(&obj),
            Some("hello direct".to_string())
        );
    }

    #[test]
    fn extract_event_msg_user_message_format() {
        let obj = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "user_message",
                "text": "hello from event_msg"
            }
        });
        assert_eq!(
            extract_codex_user_message(&obj),
            Some("hello from event_msg".to_string())
        );
    }

    #[test]
    fn extract_non_user_message_returns_none() {
        let obj = serde_json::json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "I am the assistant"}]
        });
        assert_eq!(extract_codex_user_message(&obj), None);
    }

    #[test]
    fn extract_empty_object_returns_none() {
        let obj = serde_json::json!({});
        assert_eq!(extract_codex_user_message(&obj), None);
    }

    // -----------------------------------------------------------------------
    // collect_jsonl_files
    // -----------------------------------------------------------------------

    #[test]
    fn collect_jsonl_max_depth_zero() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.jsonl"), "{}").unwrap();
        let files = collect_jsonl_files(dir.path(), 0);
        assert!(files.is_empty());
    }

    #[test]
    fn collect_jsonl_depth_one_top_level_only() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.jsonl"), "{}").unwrap();
        fs::write(dir.path().join("b.jsonl"), "{}").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/c.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(dir.path(), 1);
        assert_eq!(files.len(), 2);
        for f in &files {
            assert_eq!(f.parent().unwrap(), dir.path());
        }
    }

    #[test]
    fn collect_jsonl_ignores_non_jsonl() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.jsonl"), "{}").unwrap();
        fs::write(dir.path().join("b.txt"), "text").unwrap();
        fs::write(dir.path().join("c.json"), "{}").unwrap();

        let files = collect_jsonl_files(dir.path(), 1);
        assert_eq!(files.len(), 1);
        assert!(files[0].extension().unwrap() == "jsonl");
    }

    #[test]
    fn collect_jsonl_recursive() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.jsonl"), "{}").unwrap();
        fs::create_dir_all(dir.path().join("y/m/d")).unwrap();
        fs::write(dir.path().join("y/b.jsonl"), "{}").unwrap();
        fs::write(dir.path().join("y/m/c.jsonl"), "{}").unwrap();
        fs::write(dir.path().join("y/m/d/e.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(dir.path(), 4);
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn collect_jsonl_nonexistent_dir() {
        let files = collect_jsonl_files(Path::new("/nonexistent/path"), 3);
        assert!(files.is_empty());
    }
}
