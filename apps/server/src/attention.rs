//! Shared notification rules for desktop and Android, applied once by the
//! server's cursor-based attention feed.

use std::collections::HashMap;

use serde::Serialize;
use workbench_core::claude_transcript::WaitingSummary;

use crate::agent::{AgentKind, AgentSummary};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AttentionKind {
    /// A new approval, question or MCP form waits on an answer.
    Waiting,
    /// What it waited on was answered (or withdrawn).
    Resolved,
    /// A turn ended with nothing left waiting.
    TurnEnded,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub kind: AttentionKind,
    pub agent: AgentKind,
    pub session_id: String,
    pub previous_ids: Vec<String>,
    pub pane_id: Option<String>,
    pub terminal_id: Option<String>,
    pub project_path: String,
    pub worktree_path: Option<String>,
    pub claude_account_id: Option<String>,
    pub title: Option<String>,
    pub waiting: Option<WaitingSummary>,
    /// Still mid-turn (an answered approval lets the turn go on).
    pub busy: bool,
    /// A Codex TUI completion: open its terminal, never spawn a chat behind it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub terminal_only: bool,
}

struct Seen {
    waiting: Option<String>,
    turn_ended_at: Option<u64>,
    /// As last listed, to report it resolved should it go while waiting.
    last: AgentSummary,
}

/// The first list only seeds; a session that appears later is seeded
/// silently unless it already waits on someone.
#[derive(Default)]
pub struct AttentionTracker {
    seen: Option<HashMap<String, Seen>>,
}

impl AttentionTracker {
    pub fn update(&mut self, list: &[AgentSummary]) -> Vec<Attention> {
        let previous = self.seen.take();
        let mut next = HashMap::new();
        let mut out = Vec::new();
        let mut kept = std::collections::HashSet::new();
        for s in list {
            let old = previous.as_ref().and_then(|p| {
                std::iter::once(&s.session_id)
                    .chain(&s.previous_ids)
                    .find_map(|id| p.get_key_value(id))
            });
            if let Some((id, _)) = old {
                kept.insert(id.clone());
            }
            let old = old.map(|(_, seen)| seen);
            let waiting = s.waiting.as_ref().map(|w| w.id.clone());
            if previous.is_some() {
                let kind = if waiting.is_some() && waiting != old.and_then(|o| o.waiting.clone()) {
                    Some(AttentionKind::Waiting)
                } else if waiting.is_none() && old.is_some_and(|o| o.waiting.is_some()) {
                    Some(AttentionKind::Resolved)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    out.push(attention(kind, s));
                }
                let ended = old.is_some_and(|o| s.turn_ended_at > o.turn_ended_at);
                // The next turn's end is reported instead.
                if ended && waiting.is_none() && !s.busy && !s.exited && !s.awaiting_wake {
                    out.push(attention(AttentionKind::TurnEnded, s));
                }
            }
            next.insert(
                s.session_id.clone(),
                Seen {
                    waiting,
                    turn_ended_at: s.turn_ended_at,
                    last: s.clone(),
                },
            );
        }
        // Gone while waiting (stopped, or its process ended): nothing waits now.
        for (id, seen) in previous.iter().flatten() {
            if seen.waiting.is_some() && !kept.contains(id) {
                let gone = AgentSummary {
                    busy: false,
                    ..seen.last.clone()
                };
                out.push(attention(AttentionKind::Resolved, &gone));
            }
        }
        self.seen = Some(next);
        out
    }
}

fn attention(kind: AttentionKind, s: &AgentSummary) -> Attention {
    Attention {
        kind,
        agent: s.agent,
        session_id: s.session_id.clone(),
        previous_ids: s.previous_ids.clone(),
        pane_id: s.pane_id.clone(),
        terminal_id: s.terminal_id.clone(),
        project_path: s.project_path.clone(),
        worktree_path: s.worktree_path.clone(),
        claude_account_id: s.claude_account_id.clone(),
        title: s.title.clone(),
        waiting: s.waiting.clone(),
        busy: s.busy && !s.exited,
        terminal_only: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str) -> AgentSummary {
        AgentSummary {
            agent: AgentKind::Claude,
            session_id: id.into(),
            project_path: "/p".into(),
            worktree_path: None,
            pane_id: Some("pane".into()),
            claude_account_id: None,
            title: None,
            model: None,
            busy: false,
            exited: false,
            busy_since: None,
            updated_at: 0,
            turn_ended_at: None,
            waiting: None,
            running: None,
            running_tasks: Default::default(),
            awaiting_wake: false,
            previous_ids: Vec::new(),
            terminal_id: None,
        }
    }

    fn waiting(id: &str) -> Option<WaitingSummary> {
        Some(WaitingSummary {
            id: id.into(),
            tool: "Bash".into(),
            preview: "ls".into(),
            in_terminal: true,
        })
    }

    fn kinds(out: Vec<Attention>) -> Vec<AttentionKind> {
        out.into_iter().map(|a| a.kind).collect()
    }

    #[test]
    fn the_first_list_only_seeds() {
        let mut t = AttentionTracker::default();
        let mut s = summary("a");
        s.waiting = waiting("r1");
        s.turn_ended_at = Some(5);
        assert!(t.update(&[s.clone()]).is_empty());
        assert!(t.update(&[s]).is_empty(), "nothing changed");
    }

    #[test]
    fn approvals_and_turn_ends_are_reported_once() {
        let mut t = AttentionTracker::default();
        let mut s = summary("a");
        t.update(&[s.clone()]);
        s.busy = true;
        s.waiting = waiting("r1");
        assert_eq!(kinds(t.update(&[s.clone()])), [AttentionKind::Waiting]);
        assert!(t.update(&[s.clone()]).is_empty());
        s.waiting = None;
        assert_eq!(kinds(t.update(&[s.clone()])), [AttentionKind::Resolved]);
        s.busy = false;
        s.turn_ended_at = Some(10);
        assert_eq!(kinds(t.update(&[s.clone()])), [AttentionKind::TurnEnded]);
        assert!(t.update(&[s]).is_empty());
    }

    #[test]
    fn a_session_gone_while_waiting_is_resolved() {
        let mut t = AttentionTracker::default();
        let mut s = summary("a");
        s.waiting = waiting("r1");
        t.update(&[s, summary("b")]);
        let out = t.update(&[summary("b")]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, AttentionKind::Resolved);
        assert_eq!(out[0].session_id, "a");
        assert!(t.update(&[]).is_empty(), "b wasn't waiting");
    }

    #[test]
    fn a_new_session_alerts_only_when_it_already_waits() {
        let mut t = AttentionTracker::default();
        t.update(&[]);
        let mut s = summary("a");
        s.turn_ended_at = Some(3);
        assert!(t.update(&[s.clone()]).is_empty());
        let mut w = summary("b");
        w.waiting = waiting("r1");
        assert_eq!(kinds(t.update(&[s, w])), [AttentionKind::Waiting]);
    }

    #[test]
    fn a_clear_rekey_keeps_its_state() {
        let mut t = AttentionTracker::default();
        let mut s = summary("old");
        s.turn_ended_at = Some(3);
        t.update(&[s.clone()]);
        s.session_id = "new".into();
        s.previous_ids = vec!["old".into()];
        assert!(t.update(&[s.clone()]).is_empty());
        s.turn_ended_at = Some(4);
        assert_eq!(kinds(t.update(&[s])), [AttentionKind::TurnEnded]);
    }

    #[test]
    fn a_turn_ending_before_a_background_agent_waits_for_the_next() {
        let mut t = AttentionTracker::default();
        let mut s = summary("a");
        t.update(&[s.clone()]);
        s.turn_ended_at = Some(5);
        s.awaiting_wake = true;
        assert!(t.update(&[s.clone()]).is_empty());
        s.awaiting_wake = false;
        assert!(t.update(&[s.clone()]).is_empty(), "no turn ended since");
        s.turn_ended_at = Some(9);
        assert_eq!(kinds(t.update(&[s])), [AttentionKind::TurnEnded]);
    }

    #[test]
    fn other_background_tasks_dont_hold_a_turn_end() {
        let mut t = AttentionTracker::default();
        let mut s = summary("a");
        t.update(&[s.clone()]);
        s.turn_ended_at = Some(5);
        s.running_tasks.tasks = 1;
        assert_eq!(kinds(t.update(&[s])), [AttentionKind::TurnEnded]);
    }
}
