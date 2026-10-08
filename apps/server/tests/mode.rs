//! Switching a Claude chat's permission mode against a fake `claude` (set via
//! `WORKBENCH_CLAUDE_BIN`): the terminal restarts under the picked mode, also
//! for a new chat nobody has written to yet. Its own test binary because it
//! points `HOME` at a temp dir.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000ad";

/// Logs the mode its plugin is told and its argv to `$FAKE_CLAUDE_ARGS`, then idles.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "mode=$WORKBENCH_PERMISSION_MODE $*" >> "$FAKE_CLAUDE_ARGS"
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

#[tokio::test]
async fn a_new_chat_restarts_in_the_picked_mode() {
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
    let res = reqwest::Client::new()
        .post(format!("{base}/agent/claude"))
        .bearer_auth(TOKEN)
        .json(&json!({ "projectPath": project, "sessionId": SID }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "{}",
        res.text().await.unwrap_or_default()
    );

    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    assert_eq!(next_json(&mut ws).await["t"], "snapshot");
    // A second pick while the first restarts (a double click) must not start a
    // second `claude` on the session.
    for mode in ["plan", "acceptEdits"] {
        ws.send(Message::Text(
            json!({"t": "mode", "mode": mode}).to_string(),
        ))
        .await
        .unwrap();
    }
    // The refused second pick may be read before `replaced` closes the socket.
    let mut frame = next_json(&mut ws).await;
    if frame["t"] == "error" {
        assert!(
            frame["message"]
                .as_str()
                .unwrap()
                .contains("just restarted"),
            "{frame}"
        );
        frame = next_json(&mut ws).await;
    }
    assert_eq!(frame["t"], "replaced", "{frame}");

    let mut launches = String::new();
    for _ in 0..50 {
        launches = std::fs::read_to_string(&args).unwrap_or_default();
        if launches.lines().count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Whether the second pick is read before the socket closes is a race; give
    // a wrongly accepted one time to launch.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let launches = std::fs::read_to_string(&args).unwrap_or_default();
    let launches: Vec<&str> = launches.lines().collect();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert!(
        launches[1].contains(&format!("--permission-mode plan --session-id {SID}")),
        "{launches:?}"
    );
    assert!(launches[1].starts_with("mode=plan "), "{launches:?}");

    let res = reqwest::Client::new()
        .delete(format!("{base}/agent/claude/{SID}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    handle.stop().await;
}
