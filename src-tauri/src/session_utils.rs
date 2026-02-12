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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // is_skippable_user_message tests

    #[test]
    fn skippable_empty_string() {
        assert!(is_skippable_user_message(""));
    }

    #[test]
    fn skippable_short_string() {
        assert!(is_skippable_user_message("hi"));
        assert!(is_skippable_user_message("abcde")); // exactly 5 chars
    }

    #[test]
    fn skippable_starts_with_angle_bracket() {
        assert!(is_skippable_user_message("<system>some meta</system>"));
    }

    #[test]
    fn skippable_starts_with_request_interrupted() {
        assert!(is_skippable_user_message("[Request interrupted by user]"));
    }

    #[test]
    fn skippable_starts_with_base_directory() {
        assert!(is_skippable_user_message("Base directory: /home/user/project"));
    }

    #[test]
    fn not_skippable_normal_message() {
        assert!(!is_skippable_user_message("Please fix the bug in main.rs"));
    }

    // truncate_label tests

    #[test]
    fn truncate_label_short_unchanged() {
        assert_eq!(truncate_label("short label"), "short label");
    }

    #[test]
    fn truncate_label_long_string_truncated() {
        let long = "a".repeat(100);
        let result = truncate_label(&long);
        assert_eq!(result.len(), 80); // 77 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_label_exactly_80_unchanged() {
        let exact = "b".repeat(80);
        assert_eq!(truncate_label(&exact), exact);
    }

    #[test]
    fn truncate_label_multiline_takes_first_line() {
        let multiline = "first line\nsecond line\nthird line";
        assert_eq!(truncate_label(multiline), "first line");
    }

    // fallback_label tests

    #[test]
    fn fallback_label_normal_id() {
        let id = "abcdef1234567890";
        assert_eq!(fallback_label(id), "Session abcdef12");
    }

    #[test]
    fn fallback_label_short_id() {
        let id = "abc";
        assert_eq!(fallback_label(id), "Session abc");
    }

    // extract_text_from_content tests

    #[test]
    fn extract_text_none_returns_none() {
        assert_eq!(extract_text_from_content(None), None);
    }

    #[test]
    fn extract_text_string_value() {
        let val = json!("hello world");
        assert_eq!(
            extract_text_from_content(Some(&val)),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn extract_text_array_with_text_block() {
        let val = json!([{"type": "text", "text": "found it"}]);
        assert_eq!(
            extract_text_from_content(Some(&val)),
            Some("found it".to_string())
        );
    }

    #[test]
    fn extract_text_array_with_input_text_block() {
        let val = json!([{"type": "input_text", "text": "input found"}]);
        assert_eq!(
            extract_text_from_content(Some(&val)),
            Some("input found".to_string())
        );
    }

    #[test]
    fn extract_text_object_with_text_key() {
        let val = json!({"text": "object text"});
        assert_eq!(
            extract_text_from_content(Some(&val)),
            Some("object text".to_string())
        );
    }

    #[test]
    fn extract_text_array_no_matching_type_returns_none() {
        let val = json!([{"type": "image", "url": "http://example.com/img.png"}]);
        assert_eq!(extract_text_from_content(Some(&val)), None);
    }
}
