//! Inventory of every message kind Claude Code can write, so a CLI update
//! that adds one is noticed instead of silently dropped.
//!
//! Stream-json kinds come from the `SDKMessage` union and the inbound control
//! messages of `@anthropic-ai/claude-agent-sdk` 0.3.286 (CLI 2.1.286); the
//! rest are session-JSONL entries. When bumping the tested CLI version, diff
//! that union against these lists.

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
    "conversation_reset",
    "system:init",
    "system:status",
    "system:local_command_output",
    "system:compact_boundary",
    // JSONL only
    "attachment",
    "ai-title",
    "permission-mode",
    "system:turn_duration",
];

/// Kinds deliberately left out of the chat: bookkeeping, telemetry, or
/// already represented by another message (a tool result, the final `result`).
const IGNORED: &[&str] = &[
    "control_response",
    "keep_alive",
    "rate_limit_event",
    "tool_progress",
    "tool_use_summary",
    "auth_status",
    "prompt_suggestion",
    "system:api_retry",
    "system:control_request_progress",
    "system:model_refusal_fallback",
    "system:model_refusal_no_fallback",
    "system:hook_started",
    "system:hook_progress",
    "system:hook_response",
    "system:plugin_install",
    "system:task_notification",
    "system:task_started",
    "system:task_updated",
    "system:task_progress",
    "system:background_tasks_changed",
    "system:thinking_tokens",
    "system:session_state_changed",
    "system:worker_shutting_down",
    "system:commands_changed",
    "system:notification",
    "system:files_persisted",
    "system:memory_recall",
    "system:elicitation_complete",
    "system:permission_denied",
    "system:mirror_error",
    "system:informational",
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
    "system:stop_hook_summary",
    "system:away_summary",
    "system:bridge_status",
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
