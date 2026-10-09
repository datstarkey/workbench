//! Restarting a Claude chat (the chat menu's Restart session) against a fake
//! `claude` (set via `WORKBENCH_CLAUDE_BIN`): the terminal's `claude` is
//! relaunched under the same id and mode, clients re-attach, and nothing ends.
//! Its own test binary because it points `HOME` at a temp dir.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000ae";

/// Logs its argv to `$FAKE_CLAUDE_ARGS`, then idles.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "$*" >> "$FAKE_CLAUDE_ARGS"
while IFS= read -r line; do :; done
"#;

async fn next_json(ws: &mut (impl StreamExt<Item = Result<Message, Error>> + Unpin)) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("frame within 10s")
            .expect("stream open")
            .expect("frame ok");
        if let Message::Text(text) = frame {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

async fn launches(args: &std::path::Path, count: usize) -> Vec<String> {
    let mut text = String::new();
    for _ in 0..50 {
        text = std::fs::read_to_string(args).unwrap_or_default();
        if text.lines().count() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    text.lines().map(String::from).collect()
}

#[tokio::test]
async fn a_restart_relaunches_under_the_same_id_and_mode() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-claude.sh");
    std::fs::write(&fake, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        tmp.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project }] }).to_string(),
    )
    .unwrap();
    let args = tmp.path().join("args.log");
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_ARGS", &args);

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());

    // Into plan mode first: a restart must keep it.
    let (pane, _) = support::start_claude(&base, &project, SID).await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    assert_eq!(next_json(&mut ws).await["t"], "snapshot");
    ws.send(Message::Text(
        json!({"t": "mode", "mode": "plan"}).to_string(),
    ))
    .await
    .unwrap();
    assert_eq!(next_json(&mut ws).await["t"], "replaced");

    // Re-attach, as clients do on `replaced`, then restart.
    assert_eq!(support::start_claude(&base, &project, SID).await.0, pane);
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    assert_eq!(next_json(&mut ws).await["t"], "snapshot");
    ws.send(Message::Text(json!({"t": "restart"}).to_string()))
        .await
        .unwrap();
    let frame = next_json(&mut ws).await;
    assert_eq!(frame["t"], "replaced", "{frame}");

    let launched = launches(&args, 3).await;
    assert_eq!(launched.len(), 3, "{launched:?}");
    assert!(
        launched[2].contains(&format!("--permission-mode plan --session-id {SID}")),
        "{launched:?}"
    );
    // Not an End: the session is still there to attach to.
    assert_eq!(support::start_claude(&base, &project, SID).await.0, pane);

    support::close_pane(&base, &pane).await;
    handle.stop().await;
}
