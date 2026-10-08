use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::broadcast::error::TryRecvError;

use super::*;
use crate::agent::Launch;

fn terminal_session() -> Arc<AgentSession> {
    let req = StartAgent {
        cwd: "/tmp".into(),
        project_path: "/tmp".into(),
        worktree_path: None,
        pane_id: None,
        hook_socket: None,
        claude_account_id: None,
        launch: Launch::Claude {
            session_id: "0d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5".into(),
            permission_mode: None,
            config_dir: None,
        },
    };
    AgentSession::attach_mod(
        req,
        Driver::Claude(Default::default()),
        Arc::new(ModLink::new("t".into(), None)),
        &Arc::new(PolicyStore::default()),
        Default::default(),
    )
}

fn stream_event(event: Value) -> String {
    json!({"type": "stream_event", "event": event, "parent_tool_use_id": null}).to_string()
}

fn drain(rx: &mut broadcast::Receiver<Arc<Frame>>) -> Vec<Arc<Frame>> {
    let mut frames = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(frame) => frames.push(frame),
            Err(TryRecvError::Empty | TryRecvError::Closed) => return frames,
            Err(TryRecvError::Lagged(n)) => panic!("lagged {n} frames"),
        }
    }
}

/// A 1,000-delta reply streamed at about one delta a millisecond, as the
/// plugin posts them. Every delta used to be its own frame carrying the whole
/// item so far and the whole meta, so bytes grew with the square of its length.
#[test]
fn a_long_streamed_reply_is_sent_in_coalesced_frames_with_meta_only_on_change() {
    const DELTAS: usize = 1000;
    const CHUNK: &str = "streamed words, ";
    let session = terminal_session();
    let (_snapshot, mut rx) = session.subscribe();
    let noop = |_: &str, _: bool| true;

    session.apply_line(
        &stream_event(
            json!({"type":"message_start","message":{"id":"m1","model":"claude-opus-5-5"}}),
        ),
        noop,
    );
    session.apply_line(
        &stream_event(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
        noop,
    );
    let mut per_delta_bytes = 0;
    for _ in 0..DELTAS {
        session.apply_line(
            &stream_event(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":CHUNK}})),
            noop,
        );
        let d = lock(&session.driver);
        let view = d.view();
        let last = view.items().len() - 1;
        per_delta_bytes +=
            json!({"t": "update", "changes": [[last, &view.items()[last]]], "meta": view.meta()})
                .to_string()
                .len();
        drop(d);
        std::thread::sleep(Duration::from_millis(1));
    }
    std::thread::sleep(crate::agent::frames::FLUSH_EVERY * 4);

    let frames = drain(&mut rx);
    let bytes = |every_meta| -> usize { frames.iter().map(|f| f.render(every_meta).len()).sum() };
    let (sent, sent_legacy, count) = (bytes(false), bytes(true), frames.len());
    eprintln!(
        "{DELTAS} deltas: {count} frames, {sent} bytes ({sent_legacy} with meta on every frame); \
         one frame per delta was {per_delta_bytes} bytes"
    );
    assert!(frames.len() < DELTAS / 5, "{} frames", frames.len());
    assert!(
        sent * 10 < per_delta_bytes,
        "{sent} bytes vs {per_delta_bytes} per delta"
    );

    let parsed: Vec<Value> = frames
        .iter()
        .map(|f| serde_json::from_str(&f.render(false)).unwrap())
        .collect();
    let last = parsed.last().unwrap();
    assert_eq!(
        last["changes"][0][1]["text"],
        CHUNK.repeat(DELTAS),
        "the last frame carries the whole reply"
    );
    let with_meta = parsed.iter().filter(|f| f.get("meta").is_some()).count();
    assert!(
        with_meta <= 2,
        "meta goes out when it changes (busy), not with every frame: {with_meta}"
    );
    assert!(
        frames
            .iter()
            .all(|f| serde_json::from_str::<Value>(&f.render(true)).unwrap()["meta"].is_object()),
        "a client that didn't ask gets meta on every frame, as before"
    );
}

#[test]
fn pending_changes_go_out_before_a_later_frame() {
    let session = terminal_session();
    let (_snapshot, mut rx) = session.subscribe();
    session.apply_line(
        &stream_event(json!({"type":"message_start","message":{"id":"m1"}})),
        |_, _| true,
    );
    session.shutdown();
    assert_eq!(kinds(&mut rx), ["update", "exit"]);
}

fn kinds(rx: &mut broadcast::Receiver<Arc<Frame>>) -> Vec<String> {
    drain(rx)
        .iter()
        .map(|f| {
            let v: Value = serde_json::from_str(&f.render(false)).unwrap();
            v["t"].as_str().unwrap().to_string()
        })
        .collect()
}

#[test]
fn a_failure_is_the_last_thing_clients_read_before_the_end() {
    let session = terminal_session();
    let (_snapshot, mut rx) = session.subscribe();
    session.apply_line(
        &stream_event(json!({"type":"message_start","message":{"id":"m1"}})),
        |_, _| true,
    );
    session.fail_io("output", "line exceeds size limit");
    session.shutdown();
    // An update after the error would clear the chat's notice of it.
    assert_eq!(kinds(&mut rx), ["error", "exit"]);
}

#[test]
fn only_the_end_frame_ends_the_stream() {
    let session = terminal_session();
    let (_snapshot, mut rx) = session.subscribe();
    // A frame quoting `"t":"exit"` (a tool's input, say) is no end.
    session
        .frames
        .send(json!({"t": "commands", "commands": [{"t": "exit"}]}).to_string());
    session.shutdown();
    let ends: Vec<bool> = drain(&mut rx).iter().map(|f| f.ends()).collect();
    assert_eq!(ends, [false, true]);
}
