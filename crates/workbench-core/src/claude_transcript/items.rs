//! The chat items and session meta sent to clients (mirrored in
//! `@workbench/types` as `TranscriptItem` / `TranscriptMeta`).

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Running,
    Ok,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    Allow,
    /// Allow, and apply the CLI's suggested permission rules so it stops asking.
    AlwaysAllow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TranscriptItem {
    User {
        id: String,
        text: String,
        timestamp: String,
    },
    Text {
        id: String,
        text: String,
    },
    Thinking {
        id: String,
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    Tool {
        id: String,
        name: String,
        input: Value,
        status: ToolStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// `structuredPatch` hunks for edit tools: `{oldStart, newStart, lines}`.
        #[serde(skip_serializing_if = "Option::is_none")]
        patch: Option<Value>,
    },
    /// Claude is waiting for permission to run a tool. `id` is the request id.
    #[serde(rename_all = "camelCase")]
    Approval {
        id: String,
        tool: String,
        input: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        blocked_path: Option<String>,
        /// The CLI offered rules that would stop it asking again.
        can_always_allow: bool,
        /// The CLI withdrew the request (turn interrupted, answered elsewhere).
        expired: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        decision: Option<ApprovalDecision>,
        /// What the person chose, for `AskUserQuestion`: question text → answer.
        #[serde(skip_serializing_if = "Option::is_none")]
        answers: Option<Value>,
    },
    Notice {
        id: String,
        text: String,
    },
}

impl TranscriptItem {
    pub fn id(&self) -> &str {
        match self {
            Self::User { id, .. }
            | Self::Text { id, .. }
            | Self::Thinking { id, .. }
            | Self::Tool { id, .. }
            | Self::Approval { id, .. }
            | Self::Notice { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMeta {
    pub title: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    /// Prompt size of the latest API call (input + cache read + cache write).
    pub context_tokens: Option<u64>,
    /// A turn is in progress: set by a prompt or the first model event,
    /// cleared by `result`, the JSONL `turn_duration` line or an interrupt.
    pub busy: bool,
}
