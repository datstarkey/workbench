//! Follows the processes the panes run and folds what they do back in: a
//! plugin attaching (running), a `/clear` re-key or `/resume` (session ids),
//! chat summaries (title, busy, waiting), a Codex TUI's output (busy) and its
//! `notify` (its thread id), and exits. Then publishes a snapshot, at most
//! once per [`GAP`].

use std::time::Duration;

use tokio::time::Instant;
use workbench_core::workspace::{CodexMode, Command, Effect, PaneKind};

use super::{lock, PaneRuntime, Status, WorkspaceService};
use crate::agent::{AgentKind, AgentSummary};
use crate::terminal::TerminalMeta;

/// The least time between two snapshots.
pub(super) const GAP: Duration = Duration::from_millis(150);
/// Re-read this often anyway: busy decays and a trust dialog appears without
/// any list changing.
const TICK: Duration = Duration::from_secs(1);

pub(super) fn start(service: WorkspaceService) {
    follow_codex_notify(&service);
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("workspace service booted outside a runtime: no snapshots");
        return;
    };
    runtime.spawn(publish_loop(service));
}

/// Folds and publishes on a command, or on a process change while there are
/// panes; ticks (busy decay, a trust dialog) only while someone watches.
/// With no panes and nobody watching it sleeps until something happens.
async fn publish_loop(service: WorkspaceService) {
    let Some(ctx) = service.ctx() else {
        return;
    };
    let mut terminals = ctx.terminals.subscribe();
    let mut sessions = ctx.agents.attention.subscribe_sessions();
    let mut next = Instant::now();
    let mut pending = true;
    loop {
        let has_panes = service.has_panes();
        if pending || has_panes {
            tokio::time::sleep_until(next).await;
            let s = service.clone();
            let _ = tokio::task::spawn_blocking(move || {
                s.fold();
                s.publish();
            })
            .await;
            next = Instant::now() + GAP;
        }
        let ticking = service.has_panes() && service.watched();
        tokio::select! {
            _ = service.0.dirty.notified() => pending = true,
            _ = changed(&mut terminals) => pending = false,
            _ = changed(&mut sessions) => pending = false,
            _ = tokio::time::sleep(TICK), if ticking => pending = false,
        }
    }
}

async fn changed(rx: &mut tokio::sync::watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// A Codex TUI's thread id arrives only with its `notify` (a turn ended).
fn follow_codex_notify(service: &WorkspaceService) {
    let Some(ctx) = service.ctx() else {
        return;
    };
    let (service, mut events) = (service.clone(), ctx.agents.hooks.subscribe());
    std::thread::spawn(move || loop {
        use tokio::sync::broadcast::error::RecvError;
        let event = match events.blocking_recv() {
            Ok(event) => event,
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => break,
        };
        let crate::hook_bridge::HookEvent::Codex { pane_id, codex } = event else {
            continue;
        };
        let Some((thread, _)) = crate::hook_bridge::codex_turn_ended(&codex) else {
            continue;
        };
        let is_codex = lock(&service.0.state)
            .model
            .pane(&pane_id)
            .is_some_and(|p| p.kind == PaneKind::Codex);
        if is_codex {
            service.attach_or_stop(&pane_id, thread.to_string());
        }
    });
}

impl WorkspaceService {
    fn has_panes(&self) -> bool {
        lock(&self.0.state)
            .model
            .workspaces
            .iter()
            .any(|w| w.tabs.iter().any(|t| !t.panes.is_empty()))
    }

    /// A client follows the snapshots (beyond the publisher's own handle).
    fn watched(&self) -> bool {
        self.0.published.receiver_count() > 0
    }

    /// Fold a session the pane's process now runs; one another pane holds
    /// makes this process a second one on it, so it stops.
    fn attach_or_stop(&self, pane_id: &str, session_id: String) {
        let attached = self.fold_in(Command::SessionAttached {
            pane_id: pane_id.to_string(),
            session_id,
        });
        if let Err(e) = attached {
            tracing::warn!("stopping pane {pane_id}: {e:#}");
            self.run(vec![Effect::Stop {
                pane_id: pane_id.to_string(),
                session_id: None,
            }]);
        }
    }

    /// Read the managers and update every pane's runtime state.
    pub(super) fn fold(&self) {
        let Some(ctx) = self.ctx() else {
            return;
        };
        let terminals = ctx.terminals.list();
        let summaries = ctx.agents.summaries();
        let mut moves = Vec::new();
        let mut accounts = Vec::new();
        {
            let mut state = lock(&self.0.state);
            let panes: Vec<_> = state
                .model
                .workspaces
                .iter()
                .flat_map(|w| &w.tabs)
                .flat_map(|t| &t.panes)
                .map(|p| (p.clone(), summary_of(&summaries, p)))
                .collect();
            let mut changed = false;
            for (pane, summary) in panes {
                let rt = state.runtime.entry(pane.id.clone()).or_default();
                let before = rt.clone();
                let term = rt
                    .terminal_id
                    .as_deref()
                    .and_then(|id| terminals.iter().find(|t| t.id == id));
                match (pane.kind, summary) {
                    (_, Some(s)) => {
                        rt.status = Status::Running;
                        rt.ran = true;
                        rt.title = s.title.clone();
                        rt.busy = s.busy;
                        rt.busy_since = s.busy_since;
                        rt.turn_ended_at = s.turn_ended_at;
                        rt.running = s.running.clone();
                        let asked = s.waiting.as_ref().map(|w| &w.id);
                        if asked != rt.waiting.as_ref().map(|w| &w.id) {
                            rt.waiting_since = asked.map(|_| crate::agent::now_ms());
                        }
                        rt.waiting = s.waiting.clone();
                        if s.terminal_id.is_some() {
                            rt.terminal_id = s.terminal_id.clone();
                        }
                        if pane.session_id.as_deref() != Some(s.session_id.as_str()) {
                            moves.push((pane.id.clone(), s.clone()));
                        }
                        let account = s.claude_account_id.clone().unwrap_or_default();
                        if pane.kind == PaneKind::Claude
                            && pane.account_id.clone().unwrap_or_default() != account
                        {
                            accounts.push((pane.id.clone(), account));
                        }
                    }
                    (PaneKind::Claude, None) => {
                        rt.title = None;
                        rt.clear_activity();
                        rt.status = match term.filter(|t| t.alive) {
                            // `claude` left (`/exit`): the shell it ran in stays.
                            Some(_) if rt.ran => Status::Exited,
                            Some(t) if shows_trust(ctx, t) => Status::NeedsTrust,
                            Some(_) => Status::Starting,
                            None if rt.terminal_id.is_some() => Status::Exited,
                            None => rt.status,
                        };
                    }
                    (PaneKind::Codex, None) if pane.codex_mode == Some(CodexMode::AppServer) => {
                        if rt.ran {
                            rt.status = Status::Exited;
                            rt.clear_activity();
                        }
                    }
                    (_, None) => terminal_status(rt, term, pane.kind == PaneKind::Codex),
                }
                changed |= *rt != before;
            }
            if changed {
                state.gen += 1;
            }
        }
        // A chat's account switch: later spawns of its pane use the new login.
        for (pane_id, account_id) in accounts {
            let _ = self.fold_in(Command::AccountMoved {
                pane_id,
                account_id,
            });
        }
        for (pane_id, s) in moves {
            if s.previous_ids.is_empty() {
                self.attach_or_stop(&pane_id, s.session_id);
            } else {
                let _ = self.fold_in(Command::SessionRekeyed {
                    session_id: s.session_id,
                    previous_ids: s.previous_ids,
                });
            }
        }
    }
}

impl PaneRuntime {
    /// No session reports for the pane: nothing runs, waits or ended a turn.
    fn clear_activity(&mut self) {
        self.busy = false;
        self.busy_since = None;
        self.turn_ended_at = None;
        self.running = None;
        self.waiting = None;
        self.waiting_since = None;
    }
}

/// A shell's or Codex TUI's state is its terminal's.
fn terminal_status(rt: &mut PaneRuntime, term: Option<&TerminalMeta>, tui: bool) {
    match term {
        Some(t) if t.alive => {
            rt.status = Status::Running;
            rt.busy = tui && t.busy;
        }
        _ if rt.terminal_id.is_some() => {
            rt.status = Status::Exited;
            rt.busy = false;
        }
        _ => {}
    }
}

fn shows_trust(ctx: &super::Ctx, t: &TerminalMeta) -> bool {
    ctx.terminals
        .recent_output(&t.id)
        .is_some_and(|out| workbench_core::claude_launch::shows_trust_prompt(&out))
}

/// The live session a pane runs: by its pane id, else by its session ids.
fn summary_of<'a>(
    summaries: &'a [AgentSummary],
    pane: &workbench_core::workspace::Pane,
) -> Option<&'a AgentSummary> {
    let kind = match pane.kind {
        PaneKind::Claude => AgentKind::Claude,
        PaneKind::Codex => AgentKind::Codex,
        PaneKind::Shell => return None,
    };
    let ids: Vec<&str> = pane
        .session_id
        .iter()
        .chain(&pane.previous_ids)
        .map(String::as_str)
        .collect();
    summaries
        .iter()
        .filter(|s| s.agent == kind && !s.exited)
        .find(|s| s.pane_id.as_deref() == Some(pane.id.as_str()))
        .or_else(|| {
            summaries.iter().find(|s| {
                s.agent == kind
                    && !s.exited
                    && (ids.contains(&s.session_id.as_str())
                        || s.previous_ids.iter().any(|p| ids.contains(&p.as_str())))
            })
        })
}
