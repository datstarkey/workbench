//! A session JSONL is a tree: every entry names its `parentUuid`, and a
//! rewind (`/rewind`, or `--resume-session-at`) continues from an earlier
//! entry, leaving the old tail in the file as a dead branch. The CLI resumes
//! the branch ending at the newest entry; history must show the same one.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::BufRead;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::parse::str_at;

/// Every line of a session JSONL that parses.
pub(super) fn read_entries(path: &Path) -> Vec<Value> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

fn uuid(entry: &Value) -> Option<&str> {
    if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    str_at(entry, "uuid")
}

/// A compact boundary starts a new root but keeps its predecessor as `logicalParentUuid`.
fn parent(entry: &Value) -> Option<&str> {
    str_at(entry, "parentUuid").or_else(|| str_at(entry, "logicalParentUuid"))
}

/// A prompt a person typed: what a rewind cuts at, so the only kind of entry
/// that can start a dead branch. Tool results and injected text never do,
/// which keeps unusual but live shapes (parallel tool results) intact.
fn is_prompt(entry: &Value) -> bool {
    if str_at(entry, "type") != Some("user")
        || entry.get("isMeta").and_then(Value::as_bool) == Some(true)
    {
        return false;
    }
    match entry.pointer("/message/content") {
        Some(Value::String(_)) => true,
        Some(Value::Array(blocks)) => blocks
            .iter()
            .all(|b| str_at(b, "type") != Some("tool_result")),
        _ => false,
    }
}

/// Uuids of the entries on branches the conversation left: the subtrees under
/// a prompt that hangs off the live branch without being on it. The live
/// branch ends at `leaf`, or at the newest entry.
pub(super) fn abandoned(entries: &[Value], leaf: Option<&str>) -> HashSet<String> {
    let mut parents: HashMap<&str, &str> = HashMap::new();
    let mut children: HashMap<&str, Vec<&Value>> = HashMap::new();
    let mut last = None;
    for entry in entries {
        let Some(id) = uuid(entry) else { continue };
        if let Some(p) = parent(entry) {
            parents.insert(id, p);
            children.entry(p).or_default().push(entry);
        }
        last = Some(id);
    }
    let Some(leaf) = leaf.or(last) else {
        return HashSet::new();
    };
    let mut live = HashSet::new();
    let mut at = Some(leaf);
    while let Some(id) = at.filter(|id| live.insert(*id)) {
        at = parents.get(id).copied();
    }
    let mut stack: Vec<&str> = live
        .iter()
        .flat_map(|id| children.get(id).into_iter().flatten())
        .filter(|e| is_prompt(e))
        .filter_map(|e| uuid(e))
        .filter(|id| !live.contains(id))
        .collect();
    let mut dead = HashSet::new();
    while let Some(id) = stack.pop() {
        if dead.insert(id.to_string()) {
            stack.extend(
                children
                    .get(id)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| uuid(e)),
            );
        }
    }
    dead
}

/// The entry to resume at so the conversation continues from just before
/// the prompt `message_id`: that prompt's parent.
pub fn fork_point(path: &Path, message_id: &str) -> Result<String> {
    let entries = read_entries(path);
    let entry = entries
        .iter()
        .find(|e| uuid(e) == Some(message_id))
        .context("that message isn't in the session history")?;
    match parent(entry) {
        Some(p) => Ok(p.to_string()),
        None => bail!("Nothing comes before that message. Start a new chat instead."),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{ChatView, Transcript, TranscriptItem};
    use super::*;

    fn entry(kind: &str, id: &str, parent: Option<&str>, content: Value) -> String {
        let message = if kind == "user" {
            json!({"role": "user", "content": content})
        } else {
            json!({"id": format!("m-{id}"), "content": [content]})
        };
        json!({"type": kind, "uuid": id, "parentUuid": parent, "message": message}).to_string()
    }

    fn prompts(t: &Transcript) -> Vec<&str> {
        t.items()
            .iter()
            .filter_map(|i| match i {
                TranscriptItem::User { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn write(lines: &[String]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        fs::write(&path, lines.join("\n")).unwrap();
        (dir, path)
    }

    /// Two turns, then a rewind to before the second: the CLI continued from
    /// `a1`, leaving the second turn as a dead branch in the file.
    fn rewound() -> Vec<String> {
        let text = |t: &str| json!({"type": "text", "text": t});
        vec![
            entry("user", "u1", None, json!("first")),
            entry("assistant", "a1", Some("u1"), text("one")),
            json!({"type": "file-history-snapshot", "messageId": "u2"}).to_string(),
            entry("user", "u2", Some("a1"), json!("second")),
            entry(
                "assistant",
                "a2",
                Some("u2"),
                json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}),
            ),
            entry(
                "user",
                "r1",
                Some("a2"),
                json!([{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]),
            ),
            entry("user", "u3", Some("a1"), json!("third")),
            entry("assistant", "a3", Some("u3"), text("three")),
        ]
    }

    #[test]
    fn load_follows_the_branch_a_rewind_continued() {
        let (_dir, path) = write(&rewound());
        let t = Transcript::load(&path);
        assert_eq!(prompts(&t), ["first", "third"]);
        assert!(t
            .items()
            .iter()
            .all(|i| !matches!(i, TranscriptItem::Tool { .. })));

        // Resumed at `a1` before the CLI has written the new branch: both later turns go.
        let t = Transcript::load_at(&path, Some("a1"));
        assert_eq!(prompts(&t), ["first"]);
    }

    #[test]
    fn load_keeps_tool_results_that_branch() {
        // Older CLIs parent each parallel tool result on its own call: not a rewind.
        let call = |id: &str| json!({"type": "tool_use", "id": id, "name": "Bash", "input": {}});
        let result =
            |id: &str| json!([{"type": "tool_result", "tool_use_id": id, "content": "ok"}]);
        let (_dir, path) = write(&[
            entry("user", "u1", None, json!("go")),
            entry("assistant", "a1", Some("u1"), call("t1")),
            entry("assistant", "a2", Some("a1"), call("t2")),
            entry("user", "r1", Some("a1"), result("t1")),
            entry("user", "r2", Some("a2"), result("t2")),
        ]);
        let t = Transcript::load(&path);
        let tools = t
            .items()
            .iter()
            .filter(|i| {
                matches!(
                    i,
                    TranscriptItem::Tool {
                        output: Some(_),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(tools, 2);
    }

    #[test]
    fn fork_point_is_the_prompts_parent() {
        let (_dir, path) = write(&rewound());
        assert_eq!(fork_point(&path, "u2").unwrap(), "a1");
        assert!(
            fork_point(&path, "u1").is_err(),
            "nothing before the first prompt"
        );
        assert!(fork_point(&path, "nope").is_err());
    }
}
