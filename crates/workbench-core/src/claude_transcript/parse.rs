//! Free helpers for [`super::Transcript`]: CLI tag filtering, size caps and
//! session-file lookup.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Cap on tool output and on long string fields of tool input (e.g. a `Write`
/// body) — the chat shows a preview.
pub(crate) const MAX_TEXT_BYTES: usize = 4000;
/// Cap on diff lines kept per tool call.
const MAX_PATCH_LINES: usize = 400;

/// Tags the CLI wraps its own entries in. Only these mark a user line as
/// bookkeeping: a prompt that starts with any other tag (`<Button>`,
/// `<my-element>`) is something a person typed.
const CLI_TAGS: &[&str] = &[
    "command-name",
    "command-message",
    "command-args",
    "local-command-stdout",
    "local-command-stderr",
    "local-command-caveat",
    "system-reminder",
    "bash-input",
    "bash-stdout",
    "bash-stderr",
    "user-memory-input",
    "user-prompt-submit-hook",
    "task-notification",
];

/// What a person typed, or `None` for CLI bookkeeping. Slash commands are
/// shown as `/x …`.
pub(super) fn user_visible_text(text: &str) -> Option<UserText> {
    let Some(tag) = text
        .strip_prefix('<')
        .and_then(|rest| rest.split_once('>'))
        .map(|(tag, _)| tag)
        .filter(|tag| CLI_TAGS.contains(tag))
    else {
        return Some(UserText::Prompt(text.to_string()));
    };
    if tag != "command-name" && !text.contains("<command-name>") {
        return None;
    }
    let name = between(text, "<command-name>", "</command-name>")?;
    let args = between(text, "<command-args>", "</command-args>").unwrap_or("");
    Some(UserText::Command(
        format!("{name} {args}").trim().to_string(),
    ))
}

pub(super) enum UserText {
    /// Starts a model turn.
    Prompt(String),
    /// A slash command; may be handled locally with no turn at all.
    Command(String),
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let len = s[start..].find(close)?;
    Some(s[start..start + len].trim())
}

pub(crate) fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub(super) fn tool_output_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
            let texts: Vec<&str> = blocks.iter().filter_map(|b| str_at(b, "text")).collect();
            (!texts.is_empty()).then(|| texts.join("\n"))
        }
        _ => None,
    }
}

pub(crate) fn clip(s: &str) -> String {
    if s.len() <= MAX_TEXT_BYTES {
        return s.to_string();
    }
    format!("{}…", crate::text::truncate_bytes(s, MAX_TEXT_BYTES))
}

pub(crate) fn clip_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(clip(s)),
        Value::Array(a) => Value::Array(a.iter().map(clip_value).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), clip_value(v))).collect())
        }
        other => other.clone(),
    }
}

pub(crate) fn clip_patch(patch: &Value) -> Option<Value> {
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
            Some(json!({
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

/// Strict 8-4-4-4-12 hex check — the id becomes part of a file name and a
/// command-line argument.
pub fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.bytes().all(|b| b.is_ascii_hexdigit()))
}
