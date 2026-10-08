//! Inventory of every message kind a Claude transcript folds, so one that
//! appears unexpectedly is noticed instead of silently dropped.
//!
//! Live kinds are the SDK `SDKMessage` shapes (`@anthropic-ai/claude-agent-sdk`
//! 0.3.286, CLI 2.1.286) that the Workbench plugin translates a terminal
//! `claude` into, plus the plugin's own; the rest are session-JSONL entries.

use serde_json::Value;

use super::parse::str_at;

/// Kinds [`super::Transcript::apply`] turns into chat items or meta.
const HANDLED: &[&str] = &[
    "assistant",
    "user",
    "stream_event",
    "result",
    "control_request",
    "control_cancel_request",
    "control_response",
    "conversation_reset",
    "system:init",
    "system:local_command_output",
    "system:compact_boundary",
    "system:task_started",
    "system:task_progress",
    "system:task_updated",
    "system:task_notification",
    "rate_limit_event",
    "system:permission_denied",
    "system:model_refusal_no_fallback",
    "prompt_suggestion",
    // The plugin's own: an MCP elicitation the terminal asks, and its answer;
    // a changed slash command list.
    super::TERMINAL_ELICITATION,
    super::TERMINAL_ELICITATION_ANSWERED,
    "workbench_commands",
    // JSONL only
    "attachment",
    "ai-title",
    "custom-title",
    "permission-mode",
    "system:turn_duration",
    "system:stop_hook_summary",
];

/// Kinds deliberately left out of the chat: bookkeeping, telemetry, or
/// already represented by another message (a tool result, the final `result`).
const IGNORED: &[&str] = &[
    "keep_alive",
    "tool_progress",
    "tool_use_summary",
    "auth_status",
    "system:control_request_progress",
    "system:hook_started",
    "system:hook_progress",
    "system:plugin_install",
    "system:thinking_tokens",
    "system:session_state_changed",
    "system:worker_shutting_down",
    "system:notification",
    "system:files_persisted",
    "system:mirror_error",
    "system:informational",
    // The plugin's: names a `goal_status` row the server reads from the JSONL.
    "workbench_goal_status",
    // JSONL only
    "queue-operation",
    "file-history-snapshot",
    "file-history-delta",
    "last-prompt",
    "mode",
    "summary",
    "atis-latch",
    "bridge-session",
    "frame-link",
    "system:away_summary",
    "system:bridge_status",
    // Comment-watch bookkeeping keyed by artifact id; the links come from
    // the Artifact tool's results.
    "artifact-comment-monitor",
    "artifact-autoreact-ledger",
];

/// `type`, or `system:<subtype>` for system messages.
pub(super) fn kind(obj: &Value) -> String {
    match (str_at(obj, "type"), str_at(obj, "subtype")) {
        (Some("system"), Some(sub)) => format!("system:{sub}"),
        (Some(t), _) => t.to_string(),
        (None, _) => "<untyped>".to_string(),
    }
}

pub(super) fn is_known(kind: &str) -> bool {
    HANDLED.contains(&kind) || IGNORED.contains(&kind)
}
