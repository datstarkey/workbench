//! Codex `ThreadItem`s as chat items.

use serde_json::{json, Value};

use super::{title_from, CodexTranscript};
use crate::claude_transcript::{clip, clip_patch, clip_value, str_at, ToolStatus, TranscriptItem};

impl CodexTranscript {
    /// An item from `item/started`, `item/completed` or history. A completed
    /// item replaces what streamed in, except text that arrived only as deltas.
    pub(super) fn apply_item(
        &mut self,
        item: &Value,
        completed: bool,
        started_at_ms: Option<u64>,
        changed: &mut Vec<usize>,
    ) {
        let Some(id) = str_at(item, "id").map(String::from) else {
            return;
        };
        match str_at(item, "type") {
            Some("userMessage") => self.apply_user(id, item, started_at_ms, changed),
            Some(kind @ ("agentMessage" | "plan")) => {
                let text = bounded_text(str_at(item, "text").unwrap_or_default());
                // Started items arrive empty and fill from deltas.
                if text.is_empty() && self.index.contains_key(&id) {
                    return;
                }
                if text.is_empty() && kind == "plan" {
                    return;
                }
                self.upsert(TranscriptItem::Text { id, text }, changed);
            }
            Some("reasoning") => {
                let text = strings(item.get("summary")).join("\n\n");
                if !text.trim().is_empty() {
                    self.upsert(TranscriptItem::Thinking { id, text }, changed);
                }
            }
            Some("commandExecution") => self.apply_command(id, item, completed, changed),
            Some("fileChange") => self.apply_file_change(id, item, changed),
            Some("mcpToolCall") => {
                let name = format!(
                    "mcp__{}__{}",
                    str_at(item, "server").unwrap_or("mcp"),
                    str_at(item, "tool").unwrap_or("tool")
                );
                let output = item
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or_else(|| mcp_result_text(item.get("result")));
                if let Some(content) = item.pointer("/result/content").and_then(Value::as_array) {
                    self.keep_artifacts(&id, content);
                }
                let mut input = clip_value(item.get("arguments").unwrap_or(&Value::Null));
                if self.artifacts.contains_key(&id) {
                    if !input.is_object() {
                        input = json!({"arguments":input});
                    }
                    input["artifacts"] = json!(true);
                }
                let i = self.upsert(tool(id, name, input), changed);
                self.finish_tool(i, item_status(item, completed), output.as_deref());
            }
            Some("collabAgentToolCall") => {
                let states = item.get("agentsStates").and_then(Value::as_object);
                let receivers = item.get("receiverThreadIds").and_then(Value::as_array);
                for thread in receivers.into_iter().flatten().filter_map(Value::as_str) {
                    let state = states.and_then(|s| s.get(thread));
                    let status = state.and_then(|s| str_at(s, "status")).unwrap_or("running");
                    self.update_task(
                        thread,
                        &id,
                        str_at(item, "prompt").unwrap_or("Codex subagent"),
                        status,
                        state.and_then(|s| str_at(s, "message")),
                    );
                }
                let input = json!({"description":str_at(item,"prompt").unwrap_or("Codex subagent"),"threads":receivers,"tool":item.get("tool")});
                let i = self.upsert(tool(id, "Agent".into(), clip_value(&input)), changed);
                self.finish_tool(i, item_status(item, completed), None);
            }
            Some("subAgentActivity") => {
                let thread = str_at(item, "agentThreadId").unwrap_or(&id).to_string();
                let status = match str_at(item, "kind") {
                    Some("completed") => "completed",
                    Some("interrupted") => "stopped",
                    _ => "running",
                };
                self.update_task(
                    &thread,
                    &id,
                    str_at(item, "agentPath").unwrap_or("Codex subagent"),
                    status,
                    None,
                );
                let i = self.upsert(
                    tool(
                        id,
                        "Agent".into(),
                        json!({"description":item.get("agentPath"),"threadId":thread}),
                    ),
                    changed,
                );
                self.finish_tool(
                    i,
                    if status == "running" {
                        ToolStatus::Running
                    } else {
                        ToolStatus::Ok
                    },
                    None,
                );
            }
            Some(kind @ ("enteredReviewMode" | "exitedReviewMode")) => {
                self.upsert(
                    TranscriptItem::Notice {
                        id,
                        text: format!(
                            "{}\n{}",
                            if kind == "enteredReviewMode" {
                                "Review started"
                            } else {
                                "Review completed"
                            },
                            clip(str_at(item, "review").unwrap_or_default())
                        ),
                        in_terminal: false,
                    },
                    changed,
                );
            }
            Some("imageGeneration") => {
                let mut input = json!({"description":str_at(item,"revisedPrompt").unwrap_or("Generated image"),"file_path":item.get("savedPath")});
                if let Some(data) = str_at(item, "result") {
                    self.keep_artifacts(
                        &id,
                        &[json!({"type":"image","mimeType":"image/png","data":data})],
                    );
                }
                input["artifacts"] = json!(self.artifacts.contains_key(&id));
                let i = self.upsert(tool(id, "ImageGeneration".into(), input), changed);
                self.finish_tool(i, item_status(item, completed), str_at(item, "status"));
            }
            Some("dynamicToolCall" | "functionCallOutput") => {
                let i = self.upsert(
                    tool(
                        id,
                        str_at(item, "tool")
                            .or_else(|| str_at(item, "name"))
                            .unwrap_or("Codex tool")
                            .into(),
                        clip_value(item.get("arguments").unwrap_or(&Value::Null)),
                    ),
                    changed,
                );
                self.finish_tool(i, item_status(item, completed), str_at(item, "output"));
            }
            Some("hookPrompt") => {
                self.upsert(
                    TranscriptItem::Notice {
                        id,
                        text: format!("Hook context\n{}", clip(&item.to_string())),
                        in_terminal: false,
                    },
                    changed,
                );
            }
            Some("sleep") => {
                let i = self.upsert(tool(id, "Sleep".into(), clip_value(item)), changed);
                self.finish_tool(i, item_status(item, completed), None);
            }
            Some("webSearch") => {
                let input = json!({"query": str_at(item, "query").unwrap_or_default()});
                let i = self.upsert(tool(id, "WebSearch".into(), input), changed);
                let status = completed.then_some(ToolStatus::Ok);
                self.finish_tool(i, status.unwrap_or(ToolStatus::Running), None);
            }
            Some("imageView") => {
                let input = json!({"file_path": str_at(item, "path").unwrap_or_default()});
                let i = self.upsert(tool(id, "Read".into(), input), changed);
                self.finish_tool(i, ToolStatus::Ok, None);
            }
            Some("contextCompaction") if completed => {
                let text = "Conversation compacted".to_string();
                self.upsert(
                    TranscriptItem::Notice {
                        id,
                        text,
                        in_terminal: false,
                    },
                    changed,
                );
            }
            Some(kind) => {
                if self.unknown_seen.insert(format!("item:{kind}")) {
                    log::warn!("Codex item needs an adapter: {kind}");
                }
                self.upsert(
                    TranscriptItem::Notice {
                        id,
                        text: format!("Codex {kind}: {}", clip(&item.to_string())),
                        in_terminal: false,
                    },
                    changed,
                );
            }
            None => {}
        }
    }

    fn keep_artifacts(&mut self, id: &str, content: &[Value]) {
        use base64::Engine;
        let images: Vec<Value> = content
            .iter()
            .filter(|c| str_at(c, "type") == Some("image"))
            .filter(|c| {
                matches!(
                    str_at(c, "mimeType"),
                    Some("image/png" | "image/jpeg" | "image/webp" | "image/gif")
                )
            })
            .filter(|c| {
                str_at(c, "data").is_some_and(|data| {
                    data.len() <= 8 * 1024 * 1024
                        && base64::engine::general_purpose::STANDARD
                            .decode(data)
                            .is_ok()
                })
            })
            .take(4)
            .cloned()
            .collect();
        let retained: usize = self
            .artifacts
            .values()
            .flatten()
            .map(|c| str_at(c, "data").unwrap_or_default().len())
            .sum();
        if !images.is_empty()
            && retained
                + images
                    .iter()
                    .map(|c| str_at(c, "data").unwrap_or_default().len())
                    .sum::<usize>()
                <= 32 * 1024 * 1024
        {
            self.artifacts.insert(id.into(), images);
        }
    }

    fn update_task(
        &mut self,
        id: &str,
        tool: &str,
        description: &str,
        status: &str,
        summary: Option<&str>,
    ) {
        use crate::claude_transcript::TaskInfo;
        let status = match status {
            "completed" | "failed" | "stopped" | "paused" => status,
            "errored" => "failed",
            "shutdown" | "notFound" => "stopped",
            _ => "running",
        };
        if let Some(task) = self.meta.tasks.iter_mut().find(|t| t.id == id) {
            task.status = status.into();
            task.summary = summary.map(clip);
        } else if self.meta.tasks.len() < 200 {
            self.meta.tasks.push(TaskInfo {
                id: id.into(),
                tool_use_id: Some(tool.into()),
                kind: "agent".into(),
                description: clip(description),
                status: status.into(),
                background: true,
                summary: summary.map(clip),
                ..TaskInfo::default()
            });
        }
    }

    fn apply_user(
        &mut self,
        id: String,
        item: &Value,
        started_at_ms: Option<u64>,
        changed: &mut Vec<usize>,
    ) {
        let content = item.get("content").and_then(Value::as_array);
        let blocks = content.map(Vec::as_slice).unwrap_or_default();
        let text = blocks
            .iter()
            .filter(|b| str_at(b, "type") == Some("text"))
            .filter_map(|b| str_at(b, "text"))
            .collect::<Vec<_>>()
            .join("\n");
        let images = blocks
            .iter()
            .filter(|b| matches!(str_at(b, "type"), Some("image" | "localImage")))
            .count() as u32;
        let text = text.trim().to_string();
        if text.is_empty() && images == 0 {
            return;
        }
        if self.meta.title.is_none() && !text.is_empty() {
            self.meta.title = Some(title_from(&text));
        }
        let timestamp = started_at_ms.map(iso_utc).unwrap_or_default();
        let item = TranscriptItem::User {
            id,
            text,
            timestamp,
            images,
            files: Vec::new(),
        };
        self.upsert(item, changed);
    }

    fn apply_command(
        &mut self,
        id: String,
        item: &Value,
        completed: bool,
        changed: &mut Vec<usize>,
    ) {
        let input = json!({
            "command": strip_shell(str_at(item, "command").unwrap_or_default()),
            "cwd": str_at(item, "cwd").unwrap_or_default(),
        });
        let status = match str_at(item, "status") {
            Some("completed") => match item.get("exitCode").and_then(Value::as_i64) {
                Some(0) | None => ToolStatus::Ok,
                Some(_) => ToolStatus::Error,
            },
            Some("failed" | "declined") => ToolStatus::Error,
            _ if completed => ToolStatus::Ok,
            _ => ToolStatus::Running,
        };
        let streamed = self.live_output.get(&id).cloned();
        let output = str_at(item, "aggregatedOutput")
            .map(String::from)
            .or(streamed)
            .or_else(|| (str_at(item, "status") == Some("declined")).then(|| "Declined".into()));
        let i = self.upsert(tool(id.clone(), "Bash".into(), input), changed);
        self.finish_tool(i, status, output.as_deref());
        if completed {
            self.live_output.remove(&id);
        }
    }

    /// One `Write`/`Edit` item per changed file: the first keeps the item's
    /// id, the rest are `<id>:<n>`.
    fn apply_file_change(&mut self, id: String, item: &Value, changed: &mut Vec<usize>) {
        let changes = item
            .get("changes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let status = match str_at(item, "status") {
            Some("completed") => ToolStatus::Ok,
            Some("failed" | "declined") => ToolStatus::Error,
            _ => ToolStatus::Running,
        };
        for (n, change) in changes.iter().enumerate() {
            let tool_id = if n == 0 {
                id.clone()
            } else {
                format!("{id}:{n}")
            };
            let (name, input) = file_change_call(change);
            let patch = change_patch(change);
            let i = self.upsert(tool(tool_id, name.into(), input), changed);
            if let TranscriptItem::Tool { patch: p, .. } = &mut self.items[i] {
                *p = patch;
            }
            let output = (status == ToolStatus::Error)
                .then(|| str_at(item, "status").unwrap_or("failed").to_string());
            self.finish_tool(i, status.clone(), output.as_deref());
        }
        self.file_changes.insert(id, changes);
    }

    fn finish_tool(&mut self, i: usize, status: ToolStatus, output: Option<&str>) {
        if let TranscriptItem::Tool { status: s, .. } = &mut self.items[i] {
            *s = status;
        }
        if let Some(text) = output {
            self.set_output(i, text);
        }
    }
}

/// The tool name and input a file change shows as: `Write` for a new file
/// (`diff` is then its content), `Edit` otherwise.
pub(super) fn file_change_call(change: &Value) -> (&'static str, Value) {
    let path = str_at(change, "path").unwrap_or_default();
    let kind = change.pointer("/kind/type").and_then(Value::as_str);
    match kind {
        Some("add") => (
            "Write",
            json!({"file_path": path, "content": clip(str_at(change, "diff").unwrap_or_default())}),
        ),
        Some("delete") => ("Edit", json!({"file_path": path, "deleted": true})),
        _ => match change.pointer("/kind/move_path").and_then(Value::as_str) {
            Some(to) => ("Edit", json!({"file_path": path, "move_path": to})),
            None => ("Edit", json!({"file_path": path})),
        },
    }
}

/// `{oldStart, newStart, lines}` hunks: parsed from a unified diff, or a whole
/// file added or removed.
fn change_patch(change: &Value) -> Option<Value> {
    let diff = str_at(change, "diff").unwrap_or_default();
    let hunks = match change.pointer("/kind/type").and_then(Value::as_str) {
        _ if diff.lines().any(|l| l.starts_with("@@")) => parse_hunks(diff),
        Some(kind @ ("add" | "delete")) => {
            let (sign, old, new) = if kind == "add" {
                ('+', 0, 1)
            } else {
                ('-', 1, 0)
            };
            let lines: Vec<String> = diff.lines().map(|l| format!("{sign}{l}")).collect();
            vec![json!({"oldStart": old, "newStart": new, "lines": lines})]
        }
        _ => return None,
    };
    clip_patch(&Value::Array(hunks))
}

fn parse_hunks(diff: &str) -> Vec<Value> {
    let mut hunks: Vec<(u64, u64, Vec<String>)> = Vec::new();
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            // "@@ -74,4 +74,4 @@"
            let start = |sign: char| {
                header
                    .split_whitespace()
                    .find_map(|t| t.strip_prefix(sign))
                    .and_then(|t| t.split(',').next()?.parse().ok())
                    .unwrap_or(0)
            };
            hunks.push((start('-'), start('+'), Vec::new()));
        } else if let Some((_, _, lines)) = hunks.last_mut() {
            match line.chars().next() {
                Some('+' | '-' | ' ') => lines.push(line.to_string()),
                // Some tools drop the space of an empty context line.
                None => lines.push(" ".into()),
                _ => {} // "\ No newline at end of file"
            }
        }
    }
    hunks
        .into_iter()
        .map(|(old, new, lines)| json!({"oldStart": old, "newStart": new, "lines": lines}))
        .collect()
}

/// `/bin/zsh -lc 'echo hi'` → `echo hi`: codex wraps every command in the
/// user's shell. Left alone unless the quoting is a single plain string.
pub(super) fn strip_shell(command: &str) -> String {
    for flag in [" -lc ", " -c "] {
        let Some((shell, rest)) = command.split_once(flag) else {
            continue;
        };
        if shell.contains(' ') || !shell.ends_with("sh") {
            continue;
        }
        let inner = rest
            .strip_prefix('\'')
            .and_then(|r| r.strip_suffix('\''))
            .filter(|_| rest.len() >= 2);
        return match inner {
            Some(inner) if !inner.replace("'\\''", "").contains('\'') => {
                inner.replace("'\\''", "'")
            }
            Some(_) => command.to_string(),
            None if !rest.contains(['\'', '"', '\\']) => rest.to_string(),
            None => command.to_string(),
        };
    }
    command.to_string()
}

fn tool(id: String, name: String, input: Value) -> TranscriptItem {
    TranscriptItem::Tool {
        id,
        name,
        input,
        status: ToolStatus::Running,
        output: None,
        full_output_bytes: None,
        patch: None,
    }
}

fn item_status(item: &Value, completed: bool) -> ToolStatus {
    match str_at(item, "status") {
        Some("failed") => ToolStatus::Error,
        Some("completed") => ToolStatus::Ok,
        _ if completed => ToolStatus::Ok,
        _ => ToolStatus::Running,
    }
}

/// Unix ms as `2026-10-01T18:41:04.596Z`, the form Claude's history uses
/// (days → civil date per Howard Hinnant's `civil_from_days`).
pub(super) fn iso_utc(ms: u64) -> String {
    let secs = ms / 1000;
    let rem = secs % 86_400;
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        ms % 1000
    )
}

fn strings(v: Option<&Value>) -> Vec<&str> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Preserve text, embedded resources, resource links, and structured MCP results.
fn mcp_result_text(result: Option<&Value>) -> Option<String> {
    let result = result?;
    let mut texts = Vec::new();
    for block in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(text) = str_at(block, "text") {
            texts.push(text.to_string());
        } else if let Some(resource) = block.get("resource") {
            if let Some(text) = str_at(resource, "text") {
                texts.push(text.to_string());
            } else if let Some(uri) = str_at(resource, "uri") {
                texts.push(format!("Resource: {uri}"));
            }
        } else if let Some(uri) = str_at(block, "uri") {
            texts.push(format!(
                "{}: {uri}",
                str_at(block, "name").unwrap_or("Resource")
            ));
        }
    }
    if let Some(structured) = result.get("structuredContent").filter(|v| !v.is_null()) {
        texts.push(serde_json::to_string_pretty(structured).unwrap_or_default());
    }
    (!texts.is_empty()).then(|| texts.join("\n"))
}

fn bounded_text(text: &str) -> String {
    let mut end = text.len().min(1024 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].into()
}
