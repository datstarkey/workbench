/// Shared helpers for Claude and Codex session JSONL parsing.

const SESSION_LABEL_MAX_LENGTH: usize = 80;
const MIN_USER_MESSAGE_LENGTH: usize = 5;

/// Returns true if a user message is too short or looks like a system/meta message.
pub fn is_skippable_user_message(trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.len() <= MIN_USER_MESSAGE_LENGTH
        || trimmed.starts_with('<')
        || trimmed.starts_with("[Request interrupted")
        || trimmed.starts_with("Base directory")
}

/// Truncate a session label to the first line, capped at SESSION_LABEL_MAX_LENGTH chars.
pub fn truncate_label(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or(text);
    if first_line.len() > SESSION_LABEL_MAX_LENGTH {
        format!("{}...", &first_line[..SESSION_LABEL_MAX_LENGTH - 3])
    } else {
        first_line.to_string()
    }
}

/// Generate a fallback label from a session ID (first 8 chars).
pub fn fallback_label(session_id: &str) -> String {
    format!("Session {}", &session_id[..8.min(session_id.len())])
}

/// Extract text from a content field (string or array of content blocks).
pub fn extract_text_from_content(content: Option<&serde_json::Value>) -> Option<String> {
    match content {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Array(arr)) => arr.iter().find_map(|item| {
            let item_type = item.get("type").and_then(|v| v.as_str());
            let item_text = item.get("text").and_then(|v| v.as_str());
            if matches!(item_type, Some("text" | "input_text")) || item_type.is_none() {
                if let Some(text) = item_text {
                    return Some(text.to_string());
                }
            }
            None
        }),
        Some(serde_json::Value::Object(map)) => map
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}
