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
        /// Images attached to the message (content not kept).
        #[serde(skip_serializing_if = "is_zero")]
        images: u32,
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
        /// Size of the whole output when `output` is a preview; fetch the rest
        /// with [`super::Transcript::full_output`].
        #[serde(skip_serializing_if = "Option::is_none")]
        full_output_bytes: Option<usize>,
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

/// A slash command the session accepts (built-ins, custom commands, skills).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}

/// A model the session can switch to (from the CLI's `initialize` reply).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    /// What `set_model` takes: an alias (`opus`), `default`, or a full id.
    pub value: String,
    pub display_name: String,
    pub description: String,
    pub resolved_model: Option<String>,
    /// Effort levels it accepts; empty when it has no effort setting.
    pub effort_levels: Vec<String>,
}

/// The API call is being retried (`api_retry`): overloaded, rate limited, …
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryInfo {
    pub attempt: u64,
    pub max_retries: u64,
    pub retry_delay_ms: u64,
    /// The CLI's error kind, e.g. `overloaded`, `rate_limit`, `server_error`.
    pub error: Option<String>,
}

/// The latest usage-limit status (`rate_limit_event`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitInfo {
    /// `allowed` | `allowed_warning` | `rejected`.
    pub status: String,
    /// Unix seconds.
    pub resets_at: Option<u64>,
    /// `five_hour`, `seven_day`, …
    pub kind: Option<String>,
    /// 0–1 share of the limit used.
    pub utilization: Option<f64>,
}

/// A subagent or background job Claude started (the CLI's `task_*` events).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    pub id: String,
    /// The Task/Agent tool call that started it, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// `agent` for subagents; otherwise the CLI's task type (e.g. a shell).
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    pub description: String,
    /// `pending` | `running` | `completed` | `failed` | `stopped` | `killed` | `paused`.
    pub status: String,
    pub background: bool,
    pub tool_uses: u64,
    pub tokens: u64,
    pub duration_ms: u64,
    /// What it's doing now (`task_progress`), e.g. "Running the test suite".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
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
    /// Subagents and background jobs, in start order.
    pub tasks: Vec<TaskInfo>,
    /// Set while the CLI waits to retry a failed API call.
    pub retry: Option<RetryInfo>,
    pub rate_limit: Option<RateLimitInfo>,
    pub models: Vec<ModelOption>,
    /// The model picked in Workbench (`ModelOption::value`); `None` until one is.
    pub model_choice: Option<String>,
    /// The effort level picked in Workbench; `None` means the model's default.
    pub effort: Option<String>,
    /// The model's context window, in tokens, as the CLI reports it (Claude:
    /// each turn's `result`; `None` before the first).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Codex only: plan limits as its stream reports them (Claude's come from
    /// `GET /agent/usage`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_limits: Option<Vec<crate::claude_accounts::UsageLimit>>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}
