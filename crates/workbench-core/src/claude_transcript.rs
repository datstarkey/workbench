//! Live view of a Claude Code session as chat items.
//!
//! The interactive CLI appends every message to
//! `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl` as it happens, one
//! content block per line. [`Transcript`] folds those lines into chat items
//! (tool results update the tool call they answer), and [`TranscriptTail`]
//! follows the file so a chat view can mirror a session running in a terminal.
//! The JSONL shape is internal to the CLI, so unknown lines are ignored rather
//! than treated as errors.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

/// Cap on tool output and on long string fields of tool input (e.g. a `Write`
/// body) — the chat shows a preview; the full text is in the terminal.
const MAX_TEXT_BYTES: usize = 4000;
/// Cap on diff lines kept per tool call.
const MAX_PATCH_LINES: usize = 400;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Running,
    Ok,
    Error,
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
    /// A turn is in progress: set by a user prompt, cleared by the CLI's
    /// end-of-turn `turn_duration` line or an interrupt.
    pub busy: bool,
}

/// What one [`Transcript::apply_line`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    /// Indices into [`Transcript::items`] that were added or updated.
    pub items: Vec<usize>,
    pub meta: bool,
}

#[derive(Debug, Default)]
pub struct Transcript {
    items: Vec<TranscriptItem>,
    index: HashMap<String, usize>,
    meta: TranscriptMeta,
}

impl Transcript {
    pub fn items(&self) -> &[TranscriptItem] {
        &self.items
    }

    pub fn meta(&self) -> &TranscriptMeta {
        &self.meta
    }

    pub fn apply_line(&mut self, line: &str) -> Applied {
        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            return Applied::default();
        };
        // Subagent turns belong to their own transcript; the Task tool card
        // already stands for them here.
        if obj.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return Applied::default();
        }
        let before = self.meta.clone();
        let mut changed = Vec::new();
        match str_at(&obj, "type") {
            Some("user") => self.apply_user(&obj, &mut changed),
            Some("assistant") => self.apply_assistant(&obj, &mut changed),
            Some("attachment") => self.apply_queued_prompt(&obj, &mut changed),
            Some("ai-title") => self.meta.title = str_at(&obj, "aiTitle").map(String::from),
            Some("permission-mode") => {
                self.meta.permission_mode = str_at(&obj, "permissionMode").map(String::from)
            }
            Some("system") if str_at(&obj, "subtype") == Some("turn_duration") => {
                self.meta.busy = false
            }
            _ => {}
        }
        Applied {
            items: changed,
            meta: self.meta != before,
        }
    }

    fn apply_user(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        if obj.get("isMeta").and_then(Value::as_bool) == Some(true)
            || obj.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        {
            return;
        }
        let id = str_at(obj, "uuid").unwrap_or_default().to_string();
        let content = obj.get("message").and_then(|m| m.get("content"));
        let text = match content {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    if str_at(block, "type") == Some("tool_result") {
                        self.apply_tool_result(block, obj.get("toolUseResult"), changed);
                    }
                }
                let texts: Vec<&str> = blocks
                    .iter()
                    .filter(|b| str_at(b, "type") == Some("text"))
                    .filter_map(|b| str_at(b, "text"))
                    .collect();
                (!texts.is_empty()).then(|| texts.join("\n"))
            }
            _ => None,
        };
        let Some(text) = text.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
            return;
        };
        if text.starts_with("[Request interrupted") {
            self.meta.busy = false;
            self.upsert(
                TranscriptItem::Notice {
                    id,
                    text: "Interrupted".into(),
                },
                changed,
            );
            return;
        }
        let Some(text) = user_visible_text(text) else {
            return;
        };
        self.meta.busy = true;
        let timestamp = str_at(obj, "timestamp").unwrap_or_default().to_string();
        self.upsert(
            TranscriptItem::User {
                id,
                text,
                timestamp,
            },
            changed,
        );
    }

    /// A prompt typed while a turn was running is recorded as an attachment
    /// when the CLI folds it into that turn.
    fn apply_queued_prompt(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        let Some(att) = obj.get("attachment") else {
            return;
        };
        if str_at(att, "type") != Some("queued_command")
            || str_at(att, "commandMode") != Some("prompt")
        {
            return;
        }
        let Some(text) = str_at(att, "prompt")
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            return;
        };
        let item = TranscriptItem::User {
            id: str_at(obj, "uuid").unwrap_or_default().to_string(),
            text: text.to_string(),
            timestamp: str_at(att, "timestamp").unwrap_or_default().to_string(),
        };
        self.upsert(item, changed);
    }

    fn apply_tool_result(
        &mut self,
        block: &Value,
        result: Option<&Value>,
        changed: &mut Vec<usize>,
    ) {
        let Some(tool_id) = str_at(block, "tool_use_id") else {
            return;
        };
        let Some(&i) = self.index.get(tool_id) else {
            return;
        };
        let TranscriptItem::Tool {
            status,
            output,
            patch,
            ..
        } = &mut self.items[i]
        else {
            return;
        };
        let is_error = block.get("is_error").and_then(Value::as_bool) == Some(true);
        *status = if is_error {
            ToolStatus::Error
        } else {
            ToolStatus::Ok
        };
        *output = tool_output_text(block.get("content")).map(|t| clip(&t));
        *patch = result
            .and_then(|r| r.get("structuredPatch"))
            .and_then(clip_patch);
        changed.push(i);
    }

    fn apply_assistant(&mut self, obj: &Value, changed: &mut Vec<usize>) {
        let Some(message) = obj.get("message") else {
            return;
        };
        if let Some(model) = str_at(message, "model").filter(|m| !m.starts_with('<')) {
            self.meta.model = Some(model.to_string());
        }
        if let Some(usage) = message.get("usage") {
            let n = |k| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
            let total =
                n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
            if total > 0 {
                self.meta.context_tokens = Some(total);
            }
        }
        let Some(blocks) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        let uuid = str_at(obj, "uuid").unwrap_or_default();
        for (n, block) in blocks.iter().enumerate() {
            let id = format!("{uuid}:{n}");
            let item = match str_at(block, "type") {
                Some("text") => str_at(block, "text")
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| TranscriptItem::Text {
                        id,
                        text: t.to_string(),
                    }),
                Some("thinking") => str_at(block, "thinking")
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| TranscriptItem::Thinking {
                        id,
                        text: t.to_string(),
                    }),
                Some("tool_use") => Some(TranscriptItem::Tool {
                    id: str_at(block, "id").unwrap_or(&id).to_string(),
                    name: str_at(block, "name").unwrap_or("tool").to_string(),
                    input: block.get("input").map(clip_value).unwrap_or(Value::Null),
                    status: ToolStatus::Running,
                    output: None,
                    patch: None,
                }),
                _ => None,
            };
            if let Some(item) = item {
                self.meta.busy = true;
                self.upsert(item, changed);
            }
        }
    }

    fn upsert(&mut self, item: TranscriptItem, changed: &mut Vec<usize>) {
        let i = match self.index.get(item.id()) {
            Some(&i) => {
                // A re-written tool call must not wipe the result it already has.
                if matches!(self.items[i], TranscriptItem::Tool { .. }) {
                    return;
                }
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
    }
}

/// What a person typed, or `None` for CLI bookkeeping. Slash commands are
/// recorded as `<command-name>/x</command-name><command-args>…` and shown as
/// `/x …`; other `<tag>` payloads (command output, reminders) are hidden.
fn user_visible_text(text: &str) -> Option<String> {
    if !text.starts_with('<') {
        return Some(text.to_string());
    }
    let name = between(text, "<command-name>", "</command-name>")?;
    let args = between(text, "<command-args>", "</command-args>").unwrap_or("");
    Some(format!("{name} {args}").trim().to_string())
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let len = s[start..].find(close)?;
    Some(s[start..start + len].trim())
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn tool_output_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
            let texts: Vec<&str> = blocks.iter().filter_map(|b| str_at(b, "text")).collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

fn clip(s: &str) -> String {
    if s.len() <= MAX_TEXT_BYTES {
        return s.to_string();
    }
    format!("{}…", crate::text::truncate_bytes(s, MAX_TEXT_BYTES))
}

fn clip_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(clip(s)),
        Value::Array(a) => Value::Array(a.iter().map(clip_value).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), clip_value(v))).collect())
        }
        other => other.clone(),
    }
}

fn clip_patch(patch: &Value) -> Option<Value> {
    let hunks = patch.as_array().filter(|h| !h.is_empty())?;
    let mut budget = MAX_PATCH_LINES;
    let kept: Vec<Value> = hunks
        .iter()
        .map_while(|hunk| {
            let lines = hunk.get("lines")?.as_array()?;
            if budget == 0 {
                return None;
            }
            let take = lines.len().min(budget);
            budget -= take;
            Some(serde_json::json!({
                "oldStart": hunk.get("oldStart"),
                "newStart": hunk.get("newStart"),
                "lines": lines[..take].iter().map(clip_value).collect::<Vec<_>>(),
            }))
        })
        .collect();
    (!kept.is_empty()).then_some(Value::Array(kept))
}

/// Find a session's JSONL by id alone. Session ids are UUIDs and so unique
/// across project folders, and searching avoids re-deriving the CLI's cwd
/// encoding. `projects_dir` is `~/.claude/projects`.
pub fn find_transcript(projects_dir: &Path, session_id: &str) -> Option<PathBuf> {
    if !is_uuid(session_id) {
        return None;
    }
    let file = format!("{session_id}.jsonl");
    fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(&file))
        .find(|path| path.is_file())
}

/// Strict 8-4-4-4-12 hex check — the id becomes part of a file name.
pub fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Result of one [`TranscriptTail::poll`].
#[derive(Debug, PartialEq)]
pub enum TailUpdate {
    /// Nothing new.
    Idle,
    /// New lines were applied.
    Changed(Applied),
    /// The file shrank or was replaced; the transcript was rebuilt from scratch.
    Reset,
}

/// Follows a session JSONL as the CLI appends to it. Before the file exists
/// (a brand-new session writes it on the first prompt) polls are [`TailUpdate::Idle`].
#[derive(Debug)]
pub struct TranscriptTail {
    projects_dir: PathBuf,
    session_id: String,
    path: Option<PathBuf>,
    offset: u64,
    partial: Vec<u8>,
    transcript: Transcript,
}

impl TranscriptTail {
    pub fn new(projects_dir: PathBuf, session_id: String) -> Self {
        Self {
            projects_dir,
            session_id,
            path: None,
            offset: 0,
            partial: Vec::new(),
            transcript: Transcript::default(),
        }
    }

    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    pub fn poll(&mut self) -> std::io::Result<TailUpdate> {
        if self.path.is_none() {
            self.path = find_transcript(&self.projects_dir, &self.session_id);
        }
        let Some(path) = self.path.clone() else {
            return Ok(TailUpdate::Idle);
        };
        let len = match fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.path = None;
                return Ok(TailUpdate::Idle);
            }
            Err(e) => return Err(e),
        };
        let reset = len < self.offset;
        if reset {
            self.offset = 0;
            self.partial.clear();
            self.transcript = Transcript::default();
        }
        if len == self.offset {
            return Ok(if reset {
                TailUpdate::Reset
            } else {
                TailUpdate::Idle
            });
        }

        let mut file = fs::File::open(&path)?;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::new();
        file.take(len - self.offset).read_to_end(&mut buf)?;
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);

        // Only complete lines: the CLI may be mid-write on the last one.
        let Some(last_newline) = self.partial.iter().rposition(|&b| b == b'\n') else {
            return Ok(if reset {
                TailUpdate::Reset
            } else {
                TailUpdate::Idle
            });
        };
        let rest = self.partial.split_off(last_newline + 1);
        let complete = std::mem::replace(&mut self.partial, rest);

        let mut applied = Applied::default();
        for line in complete.split(|&b| b == b'\n') {
            let Ok(line) = std::str::from_utf8(line) else {
                continue;
            };
            if line.trim().is_empty() {
                continue;
            }
            let a = self.transcript.apply_line(line);
            applied.items.extend(a.items);
            applied.meta |= a.meta;
        }
        applied.items.sort_unstable();
        applied.items.dedup();
        Ok(if reset {
            TailUpdate::Reset
        } else if applied.items.is_empty() && !applied.meta {
            TailUpdate::Idle
        } else {
            TailUpdate::Changed(applied)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    const SID: &str = "7b3c54f4-ba22-4654-9af9-037d1cd8e555";

    fn line(v: Value) -> String {
        v.to_string()
    }

    fn user(uuid: &str, content: Value) -> String {
        line(
            json!({"type":"user","uuid":uuid,"timestamp":"2026-09-30T21:07:10Z","message":{"role":"user","content":content}}),
        )
    }

    fn assistant(uuid: &str, block: Value) -> String {
        line(
            json!({"type":"assistant","uuid":uuid,"message":{"model":"claude-opus-5-5","content":[block],
            "usage":{"input_tokens":2,"cache_read_input_tokens":100,"cache_creation_input_tokens":50}}}),
        )
    }

    #[test]
    fn folds_a_turn_into_chat_items() {
        let mut t = Transcript::default();
        t.apply_line(&user("u1", json!("Fix the keyboard inset")));
        assert!(t.meta().busy);
        t.apply_line(&assistant("a1", json!({"type":"text","text":"On it."})));
        t.apply_line(&assistant(
            "a2",
            json!({"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"bun test"}}),
        ));
        let applied = t.apply_line(&line(json!({"type":"user","uuid":"u2","message":{"content":[
            {"type":"tool_result","tool_use_id":"toolu_1","content":"41 passed","is_error":false}]}})));
        assert_eq!(applied.items, vec![2]);
        t.apply_line(&line(
            json!({"type":"system","subtype":"turn_duration","durationMs":10}),
        ));

        assert_eq!(t.items().len(), 3);
        assert!(
            matches!(&t.items()[0], TranscriptItem::User { text, .. } if text == "Fix the keyboard inset")
        );
        assert!(matches!(&t.items()[1], TranscriptItem::Text { text, .. } if text == "On it."));
        assert!(
            matches!(&t.items()[2], TranscriptItem::Tool { status: ToolStatus::Ok, output: Some(o), .. } if o == "41 passed")
        );
        assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(t.meta().context_tokens, Some(152));
        assert!(!t.meta().busy);
    }

    #[test]
    fn edit_results_carry_the_patch() {
        let mut t = Transcript::default();
        t.apply_line(&assistant(
            "a1",
            json!({"type":"tool_use","id":"toolu_e","name":"Edit","input":{"file_path":"/x.ts"}}),
        ));
        t.apply_line(&line(json!({"type":"user","uuid":"u","message":{"content":[
            {"type":"tool_result","tool_use_id":"toolu_e","content":"ok"}]},
            "toolUseResult":{"structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-a","+b"]}]}})));
        let TranscriptItem::Tool { patch: Some(p), .. } = &t.items()[0] else {
            panic!("expected patch");
        };
        assert_eq!(p[0]["lines"], json!(["-a", "+b"]));
        assert_eq!(p[0]["newStart"], json!(1));
    }

    #[test]
    fn hides_bookkeeping_and_shows_slash_commands() {
        let mut t = Transcript::default();
        t.apply_line(&line(
            json!({"type":"user","uuid":"m","isMeta":true,"message":{"content":"Base directory…"}}),
        ));
        t.apply_line(&user(
            "s",
            json!("<local-command-stdout>done</local-command-stdout>"),
        ));
        t.apply_line(&user(
            "sc",
            json!("<command-name>/compact</command-name>\n<command-args>keep tests</command-args>"),
        ));
        t.apply_line(&line(json!({"type":"user","uuid":"side","isSidechain":true,"message":{"content":"subagent prompt"}})));
        t.apply_line(&assistant("th", json!({"type":"thinking","thinking":""})));
        t.apply_line(&user("ok", json!("ok")));
        let texts: Vec<_> = t
            .items()
            .iter()
            .map(|i| match i {
                TranscriptItem::User { text, .. } => text.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(texts, vec!["/compact keep tests", "ok"]);
    }

    #[test]
    fn mid_turn_prompts_are_shown() {
        let mut t = Transcript::default();
        t.apply_line(&line(json!({"type":"attachment","uuid":"q1","attachment":{
            "type":"queued_command","prompt":"also check windows ","commandMode":"prompt","timestamp":"t"}})));
        t.apply_line(&line(
            json!({"type":"attachment","uuid":"q2","attachment":{"type":"file","prompt":"x"}}),
        ));
        assert_eq!(t.items().len(), 1);
        assert!(
            matches!(&t.items()[0], TranscriptItem::User { text, .. } if text == "also check windows")
        );
    }

    #[test]
    fn interrupt_ends_the_turn() {
        let mut t = Transcript::default();
        t.apply_line(&user("u1", json!("go")));
        t.apply_line(&user(
            "u2",
            json!([{"type":"text","text":"[Request interrupted by user]"}]),
        ));
        assert!(!t.meta().busy);
        assert!(matches!(&t.items()[1], TranscriptItem::Notice { .. }));
    }

    #[test]
    fn long_tool_input_is_clipped() {
        let mut t = Transcript::default();
        let body = "x".repeat(MAX_TEXT_BYTES * 2);
        t.apply_line(&assistant(
            "a",
            json!({"type":"tool_use","id":"toolu_w","name":"Write","input":{"content":body}}),
        ));
        let TranscriptItem::Tool { input, .. } = &t.items()[0] else {
            panic!();
        };
        assert!(input["content"].as_str().unwrap().len() <= MAX_TEXT_BYTES + 3);
    }

    #[test]
    fn uuid_check_rejects_paths() {
        assert!(is_uuid(SID));
        assert!(!is_uuid("../../etc/passwd"));
        assert!(!is_uuid("7b3c54f4-ba22-4654-9af9-037d1cd8e55"));
        assert!(!is_uuid("7b3c54f4/ba22-4654-9af9-037d1cd8e555"));
    }

    #[test]
    fn tail_follows_appends_and_waits_for_whole_lines() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("-Users-me-repo");
        fs::create_dir_all(&project).unwrap();
        let mut tail = TranscriptTail::new(dir.path().to_path_buf(), SID.into());
        assert_eq!(tail.poll().unwrap(), TailUpdate::Idle, "no file yet");

        let path = project.join(format!("{SID}.jsonl"));
        let mut f = fs::File::create(&path).unwrap();
        let first = user("u1", json!("hello there"));
        let (head, rest) = first.split_at(10);
        write!(f, "{head}").unwrap();
        assert_eq!(tail.poll().unwrap(), TailUpdate::Idle, "half a line");
        writeln!(f, "{rest}").unwrap();
        assert!(matches!(tail.poll().unwrap(), TailUpdate::Changed(a) if a.items == vec![0]));
        assert_eq!(tail.poll().unwrap(), TailUpdate::Idle);

        writeln!(f, "{}", assistant("a1", json!({"type":"text","text":"hi"}))).unwrap();
        assert!(matches!(tail.poll().unwrap(), TailUpdate::Changed(a) if a.items == vec![1]));
        assert_eq!(tail.transcript().items().len(), 2);

        fs::write(&path, format!("{}\n", user("u9", json!("fresh start")))).unwrap();
        assert_eq!(tail.poll().unwrap(), TailUpdate::Reset);
        assert_eq!(tail.transcript().items().len(), 1);
    }
}
