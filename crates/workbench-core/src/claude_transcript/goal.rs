//! `/goal`: the CLI records each change of a session's goal as a JSONL
//! `goal_status` attachment. Set: `met: false` with `sentinel`; checked and not
//! met yet: `met: false` with the evaluator's `reason`; achieved: `met: true`;
//! cleared (`/goal clear`): `met: true` with `sentinel`.
//!
//! A terminal's plugin sees the attachment without its fields, so the server
//! reads it from the session file by id ([`goal_status_entry`]) and folds that.

use std::path::Path;

use serde_json::Value;

use super::items::{GoalInfo, TranscriptItem};
use super::parse::{find_in_tail, str_at};
use super::Transcript;

pub(super) const GOAL_STATUS: &str = "goal_status";

impl Transcript {
    /// Fold a `goal_status` attachment; false when `att` is another kind.
    pub(super) fn apply_goal(
        &mut self,
        obj: &Value,
        att: &Value,
        changed: &mut Vec<usize>,
    ) -> bool {
        if str_at(att, "type") != Some(GOAL_STATUS) {
            return false;
        }
        let Some(condition) = str_at(att, "condition") else {
            return true;
        };
        let met = att.get("met").and_then(Value::as_bool) == Some(true);
        let sentinel = att.get("sentinel").and_then(Value::as_bool) == Some(true);
        if !met {
            self.meta.goal = Some(GoalInfo {
                condition: condition.to_string(),
                reason: str_at(att, "reason").map(String::from),
            });
            return true;
        }
        self.meta.goal = None;
        // A clear prints its own line; an achieved goal has only this.
        if !sentinel {
            let id = self.event_id(obj);
            self.upsert(
                TranscriptItem::Notice {
                    id,
                    text: format!("Goal achieved: {condition}"),
                    in_terminal: false,
                },
                changed,
            );
        }
        true
    }
}

/// The `goal_status` entry `uuid` near the end of a session JSONL; `None`
/// until the CLI has written it.
pub fn goal_status_entry(path: &Path, uuid: &str) -> Option<Value> {
    find_in_tail(path, |obj| {
        (str_at(&obj, "uuid") == Some(uuid)
            && obj.pointer("/attachment/type").and_then(Value::as_str) == Some(GOAL_STATUS))
        .then_some(obj)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_transcript::ChatView;
    use serde_json::json;

    const COND: &str = "tests pass";

    // As CLI 2.1.295 writes them.
    fn status(uuid: &str, att: Value) -> Value {
        json!({"type":"attachment","uuid":uuid,"timestamp":"2026-10-08T20:52:14.003Z","attachment":att})
    }
    fn set() -> Value {
        status(
            "g1",
            json!({"type":"goal_status","met":false,"sentinel":true,"condition":COND}),
        )
    }
    fn goal(t: &Transcript) -> Option<&GoalInfo> {
        t.meta().goal.as_ref()
    }

    #[test]
    fn a_goal_shows_until_met_with_the_latest_reason_it_isnt() {
        let mut t = Transcript::default();
        assert!(t.apply(&set()).meta);
        let shown = goal(&t).map(|g| (g.condition.as_str(), g.reason.as_deref()));
        assert_eq!(shown, Some((COND, None)));

        t.apply(&status(
            "g2",
            json!({"type":"goal_status","met":false,"condition":COND,"reason":"2 failing"}),
        ));
        assert_eq!(
            goal(&t).and_then(|g| g.reason.as_deref()),
            Some("2 failing")
        );
        assert!(t.items().is_empty(), "a check that fails adds no line");

        let a = t.apply(&status(
            "g3",
            json!({"type":"goal_status","met":true,"condition":COND,
                "reason":"all pass","iterations":2,"durationMs":6054,"tokens":276}),
        ));
        assert_eq!(goal(&t), None);
        assert_eq!(a.items, vec![0]);
        assert!(matches!(&t.items()[0],
            TranscriptItem::Notice { id, text, .. } if id == "g3" && text == "Goal achieved: tests pass"));
    }

    #[test]
    fn a_cleared_goal_goes_without_a_line_of_its_own() {
        let mut t = Transcript::default();
        t.apply(&set());
        t.apply(&status(
            "g2",
            json!({"type":"goal_status","met":true,"sentinel":true,"condition":COND}),
        ));
        assert_eq!(goal(&t), None);
        assert!(t.items().is_empty(), "`/goal clear` prints its own line");
    }

    #[test]
    fn clear_ends_the_goal_and_resume_takes_the_conversations_own() {
        let mut t = Transcript::default();
        t.apply(&set());
        t.apply(&json!({"type":"conversation_reset","new_conversation_id":"n"}));
        assert_eq!(goal(&t), None);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(&path, format!("{}\n", set())).unwrap();
        t.resume_history(&path);
        assert_eq!(goal(&t).map(|g| g.condition.as_str()), Some(COND));
        let loaded = Transcript::load(&path);
        assert_eq!(goal(&loaded).map(|g| g.condition.as_str()), Some(COND));
    }

    #[test]
    fn the_status_row_is_found_by_its_id_once_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let other = status("g0", json!({"type":"queued_command"}));
        std::fs::write(&path, format!("{other}\n{}\n", set())).unwrap();
        assert_eq!(goal_status_entry(&path, "g1"), Some(set()));
        assert_eq!(goal_status_entry(&path, "g0"), None, "not a goal row");
        assert_eq!(goal_status_entry(&path, "g9"), None, "not written yet");
        let missing = dir.path().join("missing.jsonl");
        assert_eq!(goal_status_entry(&missing, "g1"), None);
    }
}
