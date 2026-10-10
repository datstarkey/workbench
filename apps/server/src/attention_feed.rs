//! One notification history shared by desktop and remote clients. Cursor-based
//! long polling keeps the native Android service independent of the WebView.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::agent::AgentSummary;
use crate::attention::{Attention, AttentionTracker};
use tokio::sync::broadcast;

const RETAINED_EVENTS: usize = 512;

#[derive(Clone)]
pub struct AttentionFeed {
    state: Arc<Mutex<FeedState>>,
    changed: broadcast::Sender<()>,
    /// Bumped on every session summary change and registry change, so a
    /// the workspace fold can follow sessions without polling.
    listed: crate::changes::Changes,
}

struct FeedState {
    epoch: String,
    sequence: u64,
    next_source: u64,
    tracker: AttentionTracker,
    events: VecDeque<(u64, Attention)>,
    sessions: std::collections::HashMap<String, (u64, AgentSummary)>,
}

#[derive(Serialize)]
pub struct AttentionBatch {
    pub cursor: String,
    pub events: Vec<Attention>,
}

impl Default for AttentionFeed {
    fn default() -> Self {
        let mut tracker = AttentionTracker::default();
        tracker.update(&[]);
        Self {
            state: Arc::new(Mutex::new(FeedState {
                epoch: uuid::Uuid::new_v4().to_string(),
                sequence: 0,
                next_source: 0,
                tracker,
                events: VecDeque::new(),
                sessions: std::collections::HashMap::new(),
            })),
            changed: broadcast::channel(64).0,
            listed: Default::default(),
        }
    }
}

impl AttentionFeed {
    /// Each process gets a monotonically newer owner, so late output or exit
    /// from its predecessor cannot overwrite a replacement's notification state.
    pub(crate) fn source(&self) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.next_source += 1;
        state.next_source
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changed.subscribe()
    }

    /// Wakes whenever the session list may have changed.
    pub fn subscribe_sessions(&self) -> tokio::sync::watch::Receiver<u64> {
        self.listed.subscribe()
    }

    /// A session was registered, re-keyed or dropped.
    pub(crate) fn sessions_changed(&self) {
        self.listed.notify();
    }

    pub fn since(&self, cursor: Option<&str>) -> AttentionBatch {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .since(cursor)
    }

    /// Called synchronously when the plugin or app-server changes a session.
    /// No polling, timers or attached chat UI are involved in producing alerts.
    pub(crate) fn observe(&self, source: u64, mut session: AgentSummary) {
        if session.session_id.is_empty() {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let known = state.sessions.get(&session.session_id);
        if known.is_some_and(|(owner, _)| *owner > source) {
            return;
        }
        // Every streamed line touches its session; one that changed nothing
        // but the time wakes no one (it ran the tracker over every session and
        // refolded the workspace, under locks the long polls wait on).
        if let Some((_, old)) = known.filter(|(owner, _)| *owner == source) {
            let at = std::mem::replace(&mut session.updated_at, old.updated_at);
            if session == *old {
                return;
            }
            session.updated_at = at;
        }
        self.listed.notify();
        for alias in &session.previous_ids {
            if state
                .sessions
                .get(alias)
                .is_some_and(|(owner, _)| *owner <= source)
            {
                state.sessions.remove(alias);
            }
        }
        state
            .sessions
            .insert(session.session_id.clone(), (source, session));
        let before = state.sequence;
        state.record_sessions();
        if state.sequence != before {
            let _ = self.changed.send(());
        }
    }

    pub(crate) fn forget(&self, source: u64, id: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state
            .sessions
            .get(id)
            .is_none_or(|(owner, _)| *owner != source)
        {
            return;
        }
        self.listed.notify();
        state.sessions.remove(id);
        let before = state.sequence;
        state.record_sessions();
        if state.sequence != before {
            let _ = self.changed.send(());
        }
    }

    /// Codex terminal `notify` is already an event, with no chat driver.
    pub fn publish(&self, event: Attention) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.push(event);
        let _ = self.changed.send(());
    }
}

impl FeedState {
    fn record_sessions(&mut self) {
        let sessions: Vec<_> = self
            .sessions
            .values()
            .map(|(_, summary)| summary.clone())
            .collect();
        for event in self.tracker.update(&sessions) {
            self.push(event);
        }
    }

    fn push(&mut self, event: Attention) {
        self.sequence += 1;
        self.events.push_back((self.sequence, event));
        if self.events.len() > RETAINED_EVENTS {
            self.events.pop_front();
        }
    }

    fn since(&self, cursor: Option<&str>) -> AttentionBatch {
        let after = cursor
            .and_then(|c| c.strip_prefix(&format!("{}:", self.epoch)))
            .and_then(|n| n.parse::<u64>().ok())
            .filter(|n| *n <= self.sequence)
            .filter(|n| {
                self.events
                    .front()
                    .is_none_or(|(oldest, _)| *n >= oldest - 1)
            });
        AttentionBatch {
            cursor: format!("{}:{}", self.epoch, self.sequence),
            // Initial connections, server restarts and expired cursors seed
            // silently instead of replaying unrelated old alerts.
            events: after.map_or_else(Vec::new, |after| {
                self.events
                    .iter()
                    .filter(|(id, _)| *id > after)
                    .map(|(_, event)| event.clone())
                    .collect()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentKind;
    use crate::attention::AttentionKind;

    fn event() -> Attention {
        Attention {
            kind: AttentionKind::TurnEnded,
            agent: AgentKind::Codex,
            session_id: "thread".into(),
            previous_ids: vec![],
            pane_id: None,
            terminal_id: None,
            project_path: "/project".into(),
            worktree_path: None,
            claude_account_id: None,
            title: Some("Fix".into()),
            waiting: None,
            busy: false,
            terminal_only: false,
        }
    }

    fn summary() -> AgentSummary {
        AgentSummary {
            agent: AgentKind::Claude,
            session_id: "session".into(),
            project_path: "/project".into(),
            worktree_path: None,
            pane_id: None,
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
            previous_ids: vec![],
            terminal_id: None,
        }
    }

    #[test]
    fn each_client_reads_the_same_events_and_reconnects_without_duplicates() {
        let feed = AttentionFeed::default();
        let clone = feed.clone();
        let cursor = feed.since(None).cursor;
        feed.publish(event());
        feed.publish(event());
        let desktop = feed.since(Some(&cursor));
        let mobile = clone.since(Some(&cursor));
        assert_eq!(desktop.events.len(), 2);
        assert_eq!(desktop.cursor, mobile.cursor);
        assert_eq!(
            serde_json::to_value(desktop.events).unwrap(),
            serde_json::to_value(mobile.events).unwrap()
        );
        assert!(feed.since(Some(&desktop.cursor)).events.is_empty());
        assert!(feed.since(None).events.is_empty());
    }

    #[test]
    fn a_restart_or_an_expired_cursor_does_not_replay_old_alerts() {
        let feed = AttentionFeed::default();
        let expired = feed.since(None).cursor;
        for _ in 0..RETAINED_EVENTS {
            feed.publish(event());
        }
        let recent = feed.since(None).cursor;
        feed.publish(event());
        assert!(feed.since(Some("other-server:0")).events.is_empty());
        assert!(feed.since(Some(&expired)).events.is_empty());
        assert_eq!(feed.since(Some(&recent)).events.len(), 1);
    }

    #[test]
    fn a_line_that_changes_only_the_time_wakes_no_one() {
        let feed = AttentionFeed::default();
        let source = feed.source();
        let mut listed = feed.subscribe_sessions();
        let mut s = summary();
        feed.observe(source, s.clone());
        assert!(listed.has_changed().unwrap());
        listed.mark_unchanged();
        s.updated_at = 5;
        feed.observe(source, s.clone());
        assert!(!listed.has_changed().unwrap(), "a streamed chunk");
        s.busy = true;
        feed.observe(source, s);
        assert!(listed.has_changed().unwrap(), "a real change");
    }

    #[test]
    fn a_predecessors_late_output_or_exit_cannot_clear_its_replacement() {
        let feed = AttentionFeed::default();
        let old = feed.source();
        let new = feed.source();
        let mut s = summary();
        feed.observe(old, s.clone());
        feed.observe(new, s.clone());
        s.busy = true;
        feed.observe(new, s.clone());
        let cursor = feed.since(None).cursor;
        feed.forget(old, &s.session_id);
        let mut stale = s.clone();
        stale.busy = false;
        stale.turn_ended_at = Some(1);
        feed.observe(old, stale);
        assert!(feed.since(Some(&cursor)).events.is_empty());
        s.busy = false;
        s.turn_ended_at = Some(2);
        feed.observe(new, s);
        assert_eq!(
            feed.since(Some(&cursor)).events[0].kind,
            AttentionKind::TurnEnded
        );
    }

    #[test]
    fn only_attention_changes_wake_subscribers_and_stopping_clears_approvals() {
        use workbench_core::claude_transcript::WaitingSummary;
        let feed = AttentionFeed::default();
        let source = feed.source();
        let mut changes = feed.subscribe();
        let mut s = summary();
        feed.observe(source, s.clone());
        s.busy = true;
        feed.observe(source, s.clone());
        assert!(
            changes.try_recv().is_err(),
            "streaming state doesn't wake notification clients"
        );
        let cursor = feed.since(None).cursor;
        s.waiting = Some(WaitingSummary {
            id: "approval".into(),
            tool: "Bash".into(),
            preview: "ls".into(),
            in_terminal: false,
        });
        feed.observe(source, s.clone());
        assert!(changes.try_recv().is_ok());
        feed.forget(source, &s.session_id);
        let batch = feed.since(Some(&cursor));
        assert_eq!(
            batch.events.iter().map(|e| e.kind).collect::<Vec<_>>(),
            [AttentionKind::Waiting, AttentionKind::Resolved]
        );
    }
}
