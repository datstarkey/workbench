//! One-line forms of an approval and a running tool for the phone's session
//! list, which polls every few seconds and must not carry whole tool inputs.

use serde::Serialize;
use serde_json::Value;

use super::TranscriptItem;
use crate::text::truncate_chars;

const MAX_CHARS: usize = 240;
const PREVIEW_KEYS: &[&str] = &["command", "file_path", "notebook_path", "url", "pattern"];
const DETAIL_KEYS: &[&str] = &[
    "command",
    "file_path",
    "notebook_path",
    "url",
    "pattern",
    "description",
    "query",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WaitingSummary {
    pub id: String,
    pub tool: String,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunningSummary {
    pub name: String,
    pub detail: String,
}

impl TranscriptItem {
    /// An approval's request id, tool and what it would act on.
    pub fn waiting_summary(&self) -> Option<WaitingSummary> {
        let Self::Approval {
            id, tool, input, ..
        } = self
        else {
            return None;
        };
        let preview = first_string(input, PREVIEW_KEYS)
            .map(String::from)
            .unwrap_or_else(|| input.to_string());
        Some(WaitingSummary {
            id: id.clone(),
            tool: tool.clone(),
            preview: truncate_chars(&preview, MAX_CHARS),
        })
    }

    /// A tool call's name and what it is acting on (`""` when nothing fits).
    pub fn running_summary(&self) -> Option<RunningSummary> {
        let Self::Tool { name, input, .. } = self else {
            return None;
        };
        Some(RunningSummary {
            name: name.clone(),
            detail: truncate_chars(first_string(input, DETAIL_KEYS).unwrap_or(""), MAX_CHARS),
        })
    }
}

fn first_string<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| input.get(k)?.as_str().filter(|s| !s.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::super::ToolStatus;
    use super::*;
    use serde_json::json;

    fn approval(input: Value) -> TranscriptItem {
        TranscriptItem::Approval {
            id: "perm-1".into(),
            tool: "Bash".into(),
            input,
            description: None,
            blocked_path: None,
            can_always_allow: false,
            expired: false,
            decision: None,
            answers: None,
        }
    }

    fn tool(name: &str, input: Value) -> TranscriptItem {
        TranscriptItem::Tool {
            id: "t1".into(),
            name: name.into(),
            input,
            status: ToolStatus::Running,
            output: None,
            full_output_bytes: None,
            patch: None,
        }
    }

    #[test]
    fn waiting_previews_the_first_known_field() {
        let w = approval(json!({"description": "list", "command": "ls"}))
            .waiting_summary()
            .unwrap();
        assert_eq!(
            w,
            WaitingSummary {
                id: "perm-1".into(),
                tool: "Bash".into(),
                preview: "ls".into()
            }
        );
        let w = approval(json!({"command": " ", "file_path": "/a.rs"}))
            .waiting_summary()
            .unwrap();
        assert_eq!(w.preview, "/a.rs");
    }

    #[test]
    fn waiting_falls_back_to_compact_json() {
        let w = approval(json!({"questions": [{"q": 1}]}))
            .waiting_summary()
            .unwrap();
        assert_eq!(w.preview, r#"{"questions":[{"q":1}]}"#);
    }

    #[test]
    fn long_previews_are_cut_on_a_char_boundary() {
        let w = approval(json!({"command": "é".repeat(300)}))
            .waiting_summary()
            .unwrap();
        assert_eq!(w.preview, format!("{}…", "é".repeat(240)));
    }

    #[test]
    fn running_detail_reads_description_and_query_too() {
        let r = tool("Task", json!({"description": "explore", "prompt": "long"}))
            .running_summary()
            .unwrap();
        assert_eq!(r.name, "Task");
        assert_eq!(r.detail, "explore");
        let r = tool("WebSearch", json!({"query": "rust"}))
            .running_summary()
            .unwrap();
        assert_eq!(r.detail, "rust");
        let r = tool("TodoWrite", json!({"todos": []}))
            .running_summary()
            .unwrap();
        assert_eq!(r.detail, "");
    }

    #[test]
    fn other_items_have_no_summary() {
        assert!(tool("Bash", json!({})).waiting_summary().is_none());
        assert!(approval(json!({})).running_summary().is_none());
    }
}
