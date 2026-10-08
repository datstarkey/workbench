//! `GET /events/home`: the phone's home lists as Server-Sent Events, so it
//! follows chats and terminals without polling `GET /agent` and
//! `GET /remote/terminals`.
//!
//! Events (each `data` is the same JSON as the matching list route):
//! - `agents`: every live chat session (`GET /agent`)
//! - `terminals`: every terminal (`GET /remote/terminals`)
//! - `ping`: `{}` after 15s without another event, so a client can tell a
//!   dead connection from a quiet one (EventSource hides comment lines)
//!
//! Both lists are sent on connect, then whichever changed, at most once per
//! [`MIN_GAP`] (a streaming chat changes its summary with every chunk). A
//! shell can exit with its PTY still held open by a background child, which
//! no signal reports, so the lists are also re-read every [`REREAD`]. A stream keeps only the last lists it sent, and reads the
//! managers again only when the client takes the next event, so a slow or
//! backgrounded reader skips intermediate states instead of queueing them.
//! EventSource can't send headers, so the token may come as `?token=`. The
//! stream ends when its listener is revoked (server mode off, token rotated).

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use serde::Deserialize;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::error::ApiError;
use crate::state::{wait_revoked, AppState};

pub const PATH: &str = "/events/home";
/// The least time between two updates of one stream.
const MIN_GAP: Duration = Duration::from_secs(1);
const HEARTBEAT: Duration = Duration::from_secs(15);
const REREAD: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
pub struct HomeQuery {
    token: Option<String>,
}

pub async fn home_events(
    State(state): State<AppState>,
    Query(query): Query<HomeQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let presented = query
        .token
        .as_deref()
        .or_else(|| crate::auth::bearer(&headers));
    if !crate::auth::token_ok(&state, presented) || *state.revoked.borrow() {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "unauthorized".to_string(),
        });
    }
    let feed = Feed {
        agents: state.agents.attention.subscribe_sessions(),
        terminals: state.terminals.subscribe(),
        revoked: state.revoked.clone(),
        state,
        sent: [None, None],
        queue: VecDeque::new(),
        next_at: None,
    };
    let stream = futures_util::stream::unfold(feed, |mut feed| async move {
        feed.next().await.map(|event| (Ok(event), feed))
    });
    let ping = Event::default().event("ping").data("{}");
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(HEARTBEAT).event(ping)))
}

struct Feed {
    state: AppState,
    agents: watch::Receiver<u64>,
    terminals: watch::Receiver<u64>,
    revoked: watch::Receiver<bool>,
    /// The JSON last sent for `agents` and `terminals`.
    sent: [Option<String>; 2],
    /// Events of the last read not yet taken: at most one per list.
    queue: VecDeque<Event>,
    /// When the next read may happen; `None` before the snapshot.
    next_at: Option<Instant>,
}

impl Feed {
    /// The next event, or `None` once the listener is revoked.
    async fn next(&mut self) -> Option<Event> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Some(event);
            }
            if let Some(at) = self.next_at {
                tokio::select! {
                    _ = wait_revoked(&mut self.revoked) => return None,
                    _ = changed(&mut self.agents) => {}
                    _ = changed(&mut self.terminals) => {}
                    _ = tokio::time::sleep(REREAD) => {}
                }
                tokio::select! {
                    _ = wait_revoked(&mut self.revoked) => return None,
                    _ = tokio::time::sleep_until(at) => {}
                }
            }
            if *self.revoked.borrow() {
                return None;
            }
            // Marked seen before reading, so a change during the read wakes us again.
            self.agents.borrow_and_update();
            self.terminals.borrow_and_update();
            let (agents, terminals) = (self.state.agents.clone(), self.state.terminals.clone());
            let lists = crate::routes::blocking(move || {
                Ok([
                    ("agents", serde_json::to_string(&agents.summaries(None))?),
                    ("terminals", serde_json::to_string(&terminals.list())?),
                ])
            })
            .await;
            let lists = match lists {
                Ok(lists) => lists,
                Err(e) => {
                    tracing::warn!("home events: {}", e.message);
                    return None;
                }
            };
            self.next_at = Some(Instant::now() + MIN_GAP);
            for ((name, json), sent) in lists.into_iter().zip(&mut self.sent) {
                if sent.as_deref() != Some(json.as_str()) {
                    self.queue
                        .push_back(Event::default().event(name).data(&json));
                    *sent = Some(json);
                }
            }
        }
    }
}

/// Resolve on the next change; never, if its sender is gone.
async fn changed(rx: &mut watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}
