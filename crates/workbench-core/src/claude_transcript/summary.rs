//! One-line forms of an approval and a running tool for the phone's session
//! list, which polls every few seconds and must not carry whole tool inputs.

use serde::Serialize;
use serde_json::Value;

use super::{TaskInfo, TranscriptItem};
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
    /// Asked in the terminal's own dialog (no chat was open), so only answerable there.
    #[serde(rename = "inTerminal", skip_serializing_if = "std::ops::Not::not")]
    pub in_terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunningSummary {
    pub name: String,
    pub detail: String,
}

/// How many of a session's subagents, and of its other tasks (background
/// shells and the like), are still going; finished ones don't count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RunningTasks {
    pub agents: u32,
    pub tasks: u32,
}

impl RunningTasks {
    pub fn of(tasks: &[TaskInfo]) -> Self {
        let mut counts = Self::default();
        for task in tasks.iter().filter(|t| t.is_running()) {
            if task.kind == "agent" {
                counts.agents += 1;
            } else {
                counts.tasks += 1;
            }
        }
        counts
    }
}

impl TaskInfo {
    /// Not yet finished: chat-ui's `isRunning` counts the same statuses.
    pub fn is_running(&self) -> bool {
        matches!(self.status.as_str(), "pending" | "running" | "paused")
    }

    /// A Claude background subagent still going: when it ends, Claude Code
    /// starts another turn with its result. A background shell may never end
    /// (a dev server), so it doesn't count.
    pub fn wakes_parent(&self) -> bool {
        self.background && self.kind == "agent" && self.is_running()
    }
}

impl TranscriptItem {
    /// An approval's request id, tool and what it would act on.
    /// An MCP elicitation is `tool: "Elicitation"` with its message.
    pub fn waiting_summary(&self) -> Option<WaitingSummary> {
        if let Self::Elicitation {
            id,
            server,
            message,
            ..
        } = self
        {
            return Some(WaitingSummary {
                id: id.clone(),
                tool: "Elicitation".into(),
                preview: truncate_chars(&format!("{server}: {message}"), MAX_CHARS),
                in_terminal: false,
            });
        }
        let Self::Approval {
            id,
            tool,
            input,
            in_terminal,
            ..
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
            in_terminal: *in_terminal,
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

    #[test]
    fn running_tasks_count_unfinished_agents_and_other_tasks_apart() {
        let task = |kind: &str, status: &str| TaskInfo {
            id: format!("{kind}-{status}"),
            tool_use_id: None,
            kind: kind.into(),
            subagent_type: None,
            description: String::new(),
            status: status.into(),
            background: true,
            tool_uses: 0,
            tokens: 0,
            duration_ms: 0,
            activity: None,
            last_tool: None,
            summary: None,
            output_id: None,
        };
        let tasks = [
            task("agent", "running"),
            task("agent", "pending"),
            task("agent", "completed"),
            task("local_bash", "running"),
            task("local_bash", "paused"),
            task("local_bash", "killed"),
            task("local_bash", "failed"),
        ];
        assert_eq!(
            RunningTasks::of(&tasks),
            RunningTasks {
                agents: 2,
                tasks: 2
            }
        );
        assert_eq!(RunningTasks::of(&[]), RunningTasks::default());
    }

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
            in_terminal: false,
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
                preview: "ls".into(),
                in_terminal: false,
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
