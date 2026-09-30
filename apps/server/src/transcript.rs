//! `/claude/transcripts/:id/ws` — streams a Claude session's JSONL as chat items
//! so a client can show a chat view of a session running in a terminal.
//! Read-only: chat input is typed into the session's terminal, never sent here.
//!
//! Frames (server → client):
//! - `{"t":"snapshot","items":[…],"meta":{…},"truncated":bool}` on attach and
//!   whenever the file is rewritten.
//! - `{"t":"update","items":[…],"meta":{…}}` — added or changed items, by `id`.
//! - `{"t":"revoked"}` when the listener stops.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde_json::json;
use tokio::sync::watch;
use workbench_core::claude_transcript::{self, TailUpdate, TranscriptTail};

use crate::error::ApiError;
use crate::state::{wait_revoked, AppState};
use crate::terminal::WsAuthQuery;

const POLL: Duration = Duration::from_millis(250);
/// Items sent on attach. Older history stays in the terminal's scrollback.
const SNAPSHOT_ITEMS: usize = 300;

pub async fn transcript_attach(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    Query(auth): Query<WsAuthQuery>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    crate::auth::authorize_ws(&headers, auth.token.as_deref(), &state)?;
    if !claude_transcript::is_uuid(&session_id) {
        return Err(ApiError::bad_request("session id must be a UUID"));
    }
    let projects = workbench_core::paths::claude_user_dir().join("projects");
    let tail = TranscriptTail::new(projects, session_id);
    let revoked = state.revoked.clone();
    Ok(ws.on_upgrade(move |socket| stream(socket, tail, revoked)))
}

async fn stream(mut socket: WebSocket, tail: TranscriptTail, mut revoked: watch::Receiver<bool>) {
    let mut tail = Some(tail);
    let mut sent_snapshot = false;
    // Index of the first item the client has; older ones were cut from the
    // snapshot, and an update to one would land out of order at the bottom.
    let mut floor = 0;
    let mut interval = tokio::time::interval(POLL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                // File reads block; a first attach may parse a long session.
                let Some(mut t) = tail.take() else { return };
                let Ok((t, update)) = tokio::task::spawn_blocking(move || {
                    let update = t.poll();
                    (t, update)
                })
                .await
                else {
                    return;
                };
                let frame = match update {
                    Ok(TailUpdate::Changed(_) | TailUpdate::Idle | TailUpdate::Reset)
                        if !sent_snapshot =>
                    {
                        Some(snapshot(&t, &mut floor))
                    }
                    Ok(TailUpdate::Reset) => Some(snapshot(&t, &mut floor)),
                    Ok(TailUpdate::Changed(applied)) => {
                        let items = t.transcript().items();
                        let changed: Vec<_> = applied
                            .items
                            .iter()
                            .filter(|&&i| i >= floor)
                            .map(|&i| &items[i])
                            .collect();
                        Some(json!({"t": "update", "items": changed, "meta": t.transcript().meta()}))
                    }
                    Ok(TailUpdate::Idle) => None,
                    Err(e) => {
                        tracing::warn!("transcript read failed: {e}");
                        None
                    }
                };
                tail = Some(t);
                if let Some(frame) = frame {
                    sent_snapshot = true;
                    if socket.send(Message::Text(frame.to_string())).await.is_err() {
                        return;
                    }
                }
            }
            _ = wait_revoked(&mut revoked) => {
                let _ = socket.send(Message::Text(r#"{"t":"revoked"}"#.to_string())).await;
                let _ = socket.close().await;
                return;
            }
            msg = socket.recv() => {
                if matches!(msg, None | Some(Err(_)) | Some(Ok(Message::Close(_)))) {
                    return;
                }
            }
        }
    }
}

fn snapshot(tail: &TranscriptTail, floor: &mut usize) -> serde_json::Value {
    let items = tail.transcript().items();
    let start = items.len().saturating_sub(SNAPSHOT_ITEMS);
    *floor = start;
    json!({
        "t": "snapshot",
        "items": &items[start..],
        "meta": tail.transcript().meta(),
        "truncated": start > 0,
    })
}
