//! The chat items and session meta sent to clients (mirrored in
//! `@workbench/types` as `TranscriptItem` / `TranscriptMeta`).

use serde::{Deserialize, Serialize};
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
        /// Names of the PDFs and text files attached (content not kept).
        #[serde(skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
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
    /// An MCP server asks the person for input. `id` is the request id.
    #[serde(rename_all = "camelCase")]
    Elicitation {
        id: String,
        server: String,
        message: String,
        /// `form` (fill in `schema`) or `url` (open `url`).
        mode: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        /// Matches a URL-mode request to the server's completion notice.
        #[serde(skip_serializing)]
        elicitation_id: Option<String>,
        /// The MCP `requestedSchema`: an object of primitive fields.
        #[serde(skip_serializing_if = "Option::is_none")]
        schema: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        expired: bool,
        /// URL mode: the server reported the flow finished.
        completed: bool,
        /// Asked by the terminal's own dialog (a terminal `claude`): shown,
        /// answered there.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        in_terminal: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        action: Option<super::ElicitationAction>,
        /// What an accepted form sent.
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Notice {
        id: String,
        text: String,
        /// About something only the terminal shows (a command's panel): the
        /// chat offers to switch to it.
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        in_terminal: bool,
    },
    /// Something the CLI did around the conversation worth a line in it: a
    /// denied tool, a hook that failed or blocked, recalled memories, a refusal.
    Event {
        id: String,
        event: EventKind,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// Memory files (paths or URLs) for [`EventKind::Memory`].
        #[serde(skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    PermissionDenied,
    Hook,
    Memory,
    Refusal,
}

impl TranscriptItem {
    pub fn id(&self) -> &str {
        match self {
            Self::User { id, .. }
            | Self::Text { id, .. }
            | Self::Thinking { id, .. }
            | Self::Tool { id, .. }
            | Self::Approval { id, .. }
            | Self::Elicitation { id, .. }
            | Self::Notice { id, .. }
            | Self::Event { id, .. } => id,
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
/// Deserialized only from the server's own models cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    /// What `set_model` takes: an alias (`opus`), `default`, or a full id.
    pub value: String,
    pub display_name: String,
    pub description: String,
    pub resolved_model: Option<String>,
    /// Effort levels it accepts; empty when it has no effort setting.
    pub effort_levels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_modalities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_tiers: Vec<Value>,
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
    /// Whose `<id>.output` file holds the live output, when not `id` itself
    /// (a terminal plugin's agent task is keyed by its tool call).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_id: Option<String>,
}

/// An artifact on claude.ai that an `Artifact` call published or opened.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactInfo {
    pub tool_use_id: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `created` | `updated` | `opened` | `published`.
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMeta {
    pub title: Option<String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    /// Prompt size of the latest API call (input + cache read + cache write).
    pub context_tokens: Option<u64>,
    /// Unix ms when the prompt cache the latest API call read or wrote
    /// expires (Claude only); cleared by a compact, which leaves nothing worth keeping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_expires_at: Option<u64>,
    /// That cache's lifetime in seconds: 3600 or 300, from what the calls wrote.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_ttl_secs: Option<u64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex: Option<crate::codex_controls::State>,
    /// Artifacts this conversation's `Artifact` calls touched, in call order.
    /// Kept here because their links only arrive in the tool's structured result.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactInfo>,
    /// The CLI's guess at the next prompt; cleared when a turn starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_suggestion: Option<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}
