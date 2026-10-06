//! A Codex thread as chat items: folds the notifications and server requests
//! of `codex app-server` (JSON-RPC over stdio) into the same
//! [`TranscriptItem`]s / [`TranscriptMeta`] a Claude chat uses, so one chat
//! view renders both.
//!
//! Items map onto the Claude tool names the view already knows: a command is
//! `Bash`, a file change `Write`/`Edit` with diff hunks, a plan `TodoWrite`,
//! a question `AskUserQuestion`. An approval request becomes an approval item
//! whose answer [`CodexTranscript::resolve_approval`] turns back into the
//! JSON-RPC response. Request/response bookkeeping (ids, thread and turn
//! starts) belongs to the host; this only folds what codex says.
//!
//! The shapes are `codex app-server generate-ts` for codex-cli 0.159.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::claude_accounts::UsageLimit;
use crate::claude_transcript::{
    clip, str_at, ChatView, RetryInfo, SlashCommand, ToolStatus, TranscriptItem, TranscriptMeta,
    MAX_FULL_OUTPUT_BYTES, MAX_TEXT_BYTES,
};

mod approvals;
mod elicitation;
mod items;
mod modes;

pub use modes::{mode_of, sandbox_mode, sandbox_policy, CODEX_MODES};

/// What one [`CodexTranscript::apply`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    /// Indices into the items that were added or updated.
    pub items: Vec<usize>,
    pub meta: bool,
    /// The JSON-RPC response to a server request the host must write back now
    /// (requests chat can't serve are declined at once: unanswered, codex waits forever).
    pub reply: Option<Value>,
    /// First sighting of a method this doesn't handle.
    pub unknown_method: Option<String>,
}

/// Notifications deliberately ignored (they carry nothing the chat shows).
const IGNORED: &[&str] = &[
    "thread/started",
    "thread/status/changed",
    "thread/compacted",
    "thread/settings/updated",
    "thread/goal/updated",
    "thread/goal/cleared",
    "thread/queue/changed",
    "thread/closed",
    "turn/moderationMetadata",
    "turn/diff/updated",
    "account/updated",
    "remoteControl/status/changed",
    "mcpServer/startupStatus/updated",
    "item/fileChange/outputDelta",
    "item/fileChange/patchUpdated",
    "item/mcpToolCall/progress",
    "item/reasoning/textDelta",
    "warning",
    "configWarning",
    "deprecationNotice",
    "skills/changed",
    "hook/started",
    "hook/completed",
];

#[derive(Debug, Default)]
pub struct CodexTranscript {
    items: Vec<TranscriptItem>,
    index: HashMap<String, usize>,
    meta: TranscriptMeta,
    /// Approval item id → what answering it needs.
    approvals: HashMap<String, approvals::Pending>,
    full_outputs: HashMap<String, String>,
    /// Command output streamed so far, by item id.
    live_output: HashMap<String, String>,
    /// File changes by item id, for the approval that asks about them.
    file_changes: HashMap<String, Vec<Value>>,
    active_turn: Option<String>,
    /// The title came from the thread's name, not its first message.
    named: bool,
    unknown_seen: HashSet<String>,
    commands: Vec<SlashCommand>,
    artifacts: HashMap<String, Vec<Value>>,
}

impl ChatView for CodexTranscript {
    fn items(&self) -> &[TranscriptItem] {
        &self.items
    }
    fn meta(&self) -> &TranscriptMeta {
        &self.meta
    }
    fn commands(&self) -> &[SlashCommand] {
        &self.commands
    }
    fn full_output(&self, tool_id: &str) -> Option<&str> {
        self.full_outputs.get(tool_id).map(String::as_str)
    }
    fn waiting_on(&self) -> Option<&TranscriptItem> {
        let first = self
            .approvals
            .values()
            .map(|p| p.item)
            .filter(|&i| match &self.items[i] {
                TranscriptItem::Approval { input, .. } => {
                    input.get("isBlocking").and_then(Value::as_bool) != Some(false)
                }
                _ => true,
            })
            .min()?;
        self.items.get(first)
    }
}

impl CodexTranscript {
    /// Keep memory bounded after settled turns. Index changes require a fresh
    /// snapshot; unanswered approvals are never displaced.
    pub fn prune(&mut self) -> bool {
        if !self.approvals.is_empty() {
            return false;
        }
        let sizes: Vec<_> = self
            .items
            .iter()
            .map(|i| serde_json::to_vec(i).map(|v| v.len()).unwrap_or(0))
            .collect();
        let mut bytes: usize = sizes.iter().sum();
        let mut remove = 0;
        while self.items.len() - remove > 2
            && (self.items.len() - remove > 2000 || bytes > 32 * 1024 * 1024)
        {
            bytes = bytes.saturating_sub(sizes[remove]);
            remove += 1;
        }
        if remove == 0 {
            return false;
        }
        self.items.drain(..remove);
        self.index = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| (item.id().to_string(), i))
            .collect();
        self.full_outputs
            .retain(|id, _| self.index.contains_key(id));
        self.live_output.retain(|id, _| self.index.contains_key(id));
        self.file_changes
            .retain(|id, _| self.index.contains_key(id));
        self.artifacts.retain(|id, _| self.index.contains_key(id));
        true
    }
    pub fn artifacts(&self, id: &str) -> Option<&[Value]> {
        self.artifacts.get(id).map(Vec::as_slice)
    }
    pub fn set_codex_state(&mut self, state: crate::codex_controls::State) {
        self.meta.codex = Some(state);
    }
    pub fn set_commands(&mut self, commands: Vec<SlashCommand>) {
        self.commands = commands;
    }
    /// The turn in progress, for steering and interrupting it.
    pub fn active_turn(&self) -> Option<&str> {
        self.active_turn.as_deref()
    }

    /// Apply one message from codex: a notification, or a request (it has an `id`).
    pub fn apply(&mut self, msg: &Value) -> Applied {
        let before = self.meta.clone();
        let mut changed = Vec::new();
        let mut applied = Applied::default();
        let method = str_at(msg, "method").unwrap_or_default();
        let params = msg.get("params").unwrap_or(&Value::Null);
        if let Some(id) = msg.get("id") {
            applied.reply = self.apply_request(id, method, params, &mut changed);
        } else if !self.apply_notification(method, params, &mut changed)
            && !IGNORED.contains(&method)
            && self.unknown_seen.insert(method.to_string())
        {
            applied.unknown_method = Some(method.to_string());
        }
        applied.items = changed;
        applied.meta = self.meta != before;
        applied
    }

    /// False for a method it doesn't handle.
    fn apply_notification(
        &mut self,
        method: &str,
        params: &Value,
        changed: &mut Vec<usize>,
    ) -> bool {
        match method {
            "warning" | "configWarning" | "deprecationNotice" => {
                let message = str_at(params, "message")
                    .or_else(|| str_at(params, "summary"))
                    .unwrap_or("Codex reported a configuration warning");
                let detail = str_at(params, "details").unwrap_or_default();
                let text = if detail.is_empty() {
                    message.to_string()
                } else {
                    format!("{message}\n{detail}")
                };
                let i = self.notice(&format!("{method}:{message}"), &text);
                changed.extend(i);
            }
            "turn/diff/updated" => {
                let text = str_at(params, "diff").unwrap_or_default();
                if !text.is_empty() {
                    let i = self.notice("turn-diff", &format!("Changes in this turn\n{text}"));
                    changed.extend(i);
                }
            }
            "hook/started"
            | "hook/completed"
            | "mcpServer/startupStatus/updated"
            | "item/mcpToolCall/progress" => {
                let id = str_at(params, "itemId")
                    .or_else(|| str_at(params, "hookId"))
                    .or_else(|| str_at(params, "server"))
                    .unwrap_or(method);
                let message = str_at(params, "message")
                    .or_else(|| str_at(params, "status"))
                    .unwrap_or(method);
                let i = self.notice(&format!("progress:{id}"), message);
                changed.extend(i);
            }
            "turn/started" => {
                let turn = params.pointer("/turn/id").and_then(Value::as_str);
                self.turn_started(turn.unwrap_or_default());
            }
            "turn/completed" => self.turn_completed(params.get("turn"), changed),
            "item/started" | "item/completed" => {
                if let Some(item) = params.get("item") {
                    let at = params.get("startedAtMs").or(params.get("completedAtMs"));
                    let at = at.and_then(Value::as_u64);
                    self.apply_item(item, method == "item/completed", at, changed);
                }
            }
            "item/agentMessage/delta" | "item/plan/delta" => {
                self.append_text(params, false, changed)
            }
            "item/reasoning/summaryTextDelta" => self.append_text(params, true, changed),
            "item/reasoning/summaryPartAdded" => {
                if let Some(&i) = str_at(params, "itemId").and_then(|id| self.index.get(id)) {
                    if let TranscriptItem::Thinking { text, .. } = &mut self.items[i] {
                        if !text.is_empty() {
                            text.push_str("\n\n");
                        }
                    }
                }
            }
            "item/commandExecution/outputDelta" => self.append_output(params, changed),
            "serverRequest/resolved" => {
                if let Some(id) = params.get("requestId") {
                    self.expire_request(id, changed);
                }
            }
            "thread/tokenUsage/updated" => {
                let usage = params.get("tokenUsage").unwrap_or(&Value::Null);
                let n = |p| usage.pointer(p).and_then(Value::as_u64);
                if let Some(tokens) = n("/last/inputTokens") {
                    self.meta.context_tokens = Some(tokens);
                }
                if let Some(window) = n("/modelContextWindow") {
                    self.meta.context_window = Some(window);
                }
            }
            "account/rateLimits/updated" => {
                if let Some(limits) = params.get("rateLimits") {
                    self.apply_rate_limits(limits);
                }
            }
            "turn/plan/updated" => self.apply_plan(params, changed),
            "error" => self.apply_error(params, changed),
            "thread/name/updated" => {
                if let Some(name) = str_at(params, "threadName").filter(|n| !n.trim().is_empty()) {
                    self.meta.title = Some(title_from(name));
                    self.named = true;
                }
            }
            "model/rerouted" => {
                if let Some(model) = str_at(params, "toModel") {
                    self.meta.model = Some(model.to_string());
                }
            }
            _ => return false,
        }
        true
    }

    /// A `thread/start` or `thread/resume` result: the model, the effective
    /// preset and the thread's name.
    pub fn apply_thread(&mut self, result: &Value) {
        self.meta.effort = str_at(result, "reasoningEffort").map(String::from);
        let state = self.meta.codex.get_or_insert_with(Default::default);
        state.approval_policy = result.get("approvalPolicy").cloned();
        state.sandbox = result.get("sandbox").cloned();
        state.service_tier = str_at(result, "serviceTier").map(String::from);
        if let Some(model) = str_at(result, "model") {
            self.meta.model = Some(model.to_string());
        }
        let sandbox = result.pointer("/sandbox/type").and_then(Value::as_str);
        self.meta.permission_mode = result
            .get("approvalPolicy")
            .zip(sandbox)
            .and_then(|(policy, sandbox)| mode_of(policy, sandbox))
            .map(String::from);
        let thread = result.get("thread").unwrap_or(&Value::Null);
        if let Some(name) = str_at(thread, "name").filter(|n| !n.trim().is_empty()) {
            self.meta.title = Some(title_from(name));
            self.named = true;
        } else if let Some(preview) = str_at(thread, "preview").filter(|p| !p.trim().is_empty()) {
            self.meta.title = Some(title_from(preview));
        }
    }

    /// History from `thread/items/list` entries (`{item, startedAtMs}`), oldest first.
    pub fn load_history(&mut self, entries: &[Value]) {
        let mut changed = Vec::new();
        for entry in entries {
            if let Some(item) = entry.get("item") {
                let at = entry.get("startedAtMs").and_then(Value::as_u64);
                self.apply_item(item, true, at, &mut changed);
            }
        }
        self.meta.busy = false;
        self.active_turn = None;
    }

    /// `model/list` data: the models the picker offers.
    pub fn set_models(&mut self, data: &[Value]) {
        self.meta.models = modes::model_options(data);
    }

    /// Mark a prompt as sent before codex starts the turn.
    pub fn set_busy(&mut self) {
        self.meta.busy = true;
    }

    pub fn set_permission_mode(&mut self, mode: &str) {
        self.meta.permission_mode = Some(mode.to_string());
    }

    pub fn set_model_choice(&mut self, value: &str) {
        self.meta.model_choice = Some(value.to_string());
        let resolved = self.meta.models.iter().find(|m| m.value == value);
        if let Some(model) = resolved.and_then(|m| m.resolved_model.clone()) {
            self.meta.model = Some(model);
        }
    }

    pub fn set_effort(&mut self, level: Option<&str>) {
        self.meta.effort = level.map(String::from);
    }

    /// A notice the host raises itself, e.g. a rejected `turn/start`.
    pub fn notice(&mut self, id: &str, text: &str) -> Vec<usize> {
        let mut changed = Vec::new();
        let item = TranscriptItem::Notice {
            id: id.to_string(),
            text: clip(text),
        };
        self.upsert(item, &mut changed);
        changed
    }

    /// The turn codex started (from `turn/started` or the `turn/start` result).
    pub fn turn_started(&mut self, turn_id: &str) {
        self.active_turn = Some(turn_id.to_string());
        self.meta.busy = true;
        self.meta.retry = None;
    }

    /// No turn is running after all (`turn/start` was refused).
    pub fn set_idle(&mut self) {
        self.active_turn = None;
        self.meta.busy = false;
        self.meta.retry = None;
    }

    fn turn_completed(&mut self, turn: Option<&Value>, changed: &mut Vec<usize>) {
        self.set_idle();
        // Codex has given up on anything still waiting for an answer.
        self.expire_all(changed);
        let Some(turn) = turn else {
            return;
        };
        let id = str_at(turn, "id").unwrap_or_default();
        match str_at(turn, "status") {
            Some("failed") => {
                let text = turn
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("The turn failed.");
                let item = TranscriptItem::Notice {
                    id: format!("turn-error:{id}"),
                    text: clip(text),
                };
                self.upsert(item, changed);
            }
            Some("interrupted") => {
                let item = TranscriptItem::Notice {
                    id: format!("interrupted:{id}"),
                    text: "Interrupted".into(),
                };
                self.upsert(item, changed);
            }
            _ => {}
        }
    }

    /// `error`: a retry shows in the activity line; a final one as a notice.
    fn apply_error(&mut self, params: &Value, changed: &mut Vec<usize>) {
        let message = params
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Codex reported an error.");
        if params.get("willRetry").and_then(Value::as_bool) == Some(true) {
            // "Reconnecting... 2/5"
            let (attempt, max_retries) = message
                .rsplit(' ')
                .next()
                .and_then(|t| t.split_once('/'))
                .and_then(|(a, m)| Some((a.parse().ok()?, m.parse().ok()?)))
                .unwrap_or((0, 0));
            self.meta.retry = Some(RetryInfo {
                attempt,
                max_retries,
                retry_delay_ms: 0,
                error: Some(message.to_string()),
            });
            return;
        }
        let turn = str_at(params, "turnId").unwrap_or_default();
        let item = TranscriptItem::Notice {
            id: format!("turn-error:{turn}"),
            text: clip(message),
        };
        self.upsert(item, changed);
    }

    /// Sparse: a window missing from an update keeps its last value.
    fn apply_rate_limits(&mut self, limits: &Value) {
        // Other ids are per-model quotas, not the plan's.
        if str_at(limits, "limitId").is_some_and(|id| id != "codex") {
            return;
        }
        let list = self.meta.usage_limits.get_or_insert_with(Vec::new);
        for (key, label) in [("primary", "session"), ("secondary", "week (all models)")] {
            let Some(window) = limits.get(key).filter(|w| w.is_object()) else {
                continue;
            };
            let Some(used) = window.get("usedPercent").and_then(Value::as_f64) else {
                continue;
            };
            let limit = UsageLimit {
                label: label.to_string(),
                percent: used.round().clamp(0.0, 100.0) as u8,
                resets: None,
                resets_at: window.get("resetsAt").and_then(Value::as_u64),
            };
            match list.iter_mut().find(|l| l.label == label) {
                Some(existing) => *existing = limit,
                None => list.push(limit),
            }
        }
    }

    /// The plan, as the `TodoWrite` call the chat's plan panel reads.
    fn apply_plan(&mut self, params: &Value, changed: &mut Vec<usize>) {
        let Some(steps) = params.get("plan").and_then(Value::as_array) else {
            return;
        };
        let todos: Vec<Value> = steps
            .iter()
            .map(|s| {
                let status = match str_at(s, "status") {
                    Some("completed") => "completed",
                    Some("inProgress") => "in_progress",
                    _ => "pending",
                };
                json!({"content": str_at(s, "step").unwrap_or_default(), "status": status})
            })
            .collect();
        let turn = str_at(params, "turnId").unwrap_or_default();
        let item = TranscriptItem::Tool {
            id: format!("plan:{turn}"),
            name: "TodoWrite".into(),
            input: json!({ "todos": todos }),
            status: ToolStatus::Ok,
            output: None,
            full_output_bytes: None,
            patch: None,
        };
        self.upsert(item, changed);
    }

    /// Stream a delta into a text (or reasoning) item, creating it if need be.
    fn append_text(&mut self, params: &Value, thinking: bool, changed: &mut Vec<usize>) {
        let (Some(id), Some(delta)) = (str_at(params, "itemId"), str_at(params, "delta")) else {
            return;
        };
        let i = match self.index.get(id) {
            Some(&i) => i,
            None => {
                let id = id.to_string();
                let text = String::new();
                let item = if thinking {
                    TranscriptItem::Thinking { id, text }
                } else {
                    TranscriptItem::Text { id, text }
                };
                self.upsert(item, changed)
            }
        };
        if let TranscriptItem::Text { text, .. } | TranscriptItem::Thinking { text, .. } =
            &mut self.items[i]
        {
            let remaining = (1024 * 1024_usize).saturating_sub(text.len());
            let mut end = delta.len().min(remaining);
            while !delta.is_char_boundary(end) {
                end -= 1;
            }
            text.push_str(&delta[..end]);
            changed.push(i);
        }
    }

    fn append_output(&mut self, params: &Value, changed: &mut Vec<usize>) {
        let (Some(id), Some(delta)) = (str_at(params, "itemId"), str_at(params, "delta")) else {
            return;
        };
        let Some(&i) = self.index.get(id) else {
            return;
        };
        // Only the head shows while it streams (the whole output comes with
        // the completed item), so stop once the preview is full rather than
        // re-copying an ever-growing buffer on every delta.
        let buffer = self.live_output.entry(id.to_string()).or_default();
        if buffer.len() > MAX_TEXT_BYTES {
            return;
        }
        buffer.push_str(delta);
        let preview = clip(buffer);
        if let TranscriptItem::Tool { output, .. } = &mut self.items[i] {
            *output = Some(preview);
            changed.push(i);
        }
    }

    /// Show `text` as a tool's output: a preview, with the whole kept for
    /// "show full output" when it's long.
    fn set_output(&mut self, i: usize, text: &str) {
        let TranscriptItem::Tool {
            id,
            output,
            full_output_bytes,
            ..
        } = &mut self.items[i]
        else {
            return;
        };
        *output = (!text.is_empty()).then(|| clip(text));
        *full_output_bytes = None;
        if text.len() > MAX_TEXT_BYTES {
            let kept = crate::text::truncate_bytes(text, MAX_FULL_OUTPUT_BYTES).to_string();
            self.full_outputs.remove(id);
            let total: usize = self.full_outputs.values().map(String::len).sum();
            if total + kept.len() <= 32 * 1024 * 1024 {
                *full_output_bytes = Some(text.len());
                self.full_outputs.insert(id.clone(), kept);
            }
        }
    }

    /// Add an item, or replace the one with its id. Returns its index.
    fn upsert(&mut self, item: TranscriptItem, changed: &mut Vec<usize>) -> usize {
        let i = match self.index.get(item.id()) {
            Some(&i) => {
                self.items[i] = item;
                i
            }
            None => {
                self.index.insert(item.id().to_string(), self.items.len());
                self.items.push(item);
                self.items.len() - 1
            }
        };
        changed.push(i);
        i
    }
}

/// A chat title from a name or first message: its first line, kept short.
fn title_from(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or_default().trim();
    crate::text::truncate_chars(line, 80)
}

#[cfg(test)]
mod tests;
