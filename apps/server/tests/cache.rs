//! A Claude chat's prompt cache controls against a fake `claude` (set via
//! `WORKBENCH_CLAUDE_BIN`): a policy is broadcast, saved and restored with the
//! session, a too-long keep-warm is refused, and a ping sends the keep-alive
//! prompt. Its own test binary because it points `HOME` at a temp dir.
#![cfg(unix)]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_core::claude_transcript::KEEPALIVE_PROMPT;
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000ca";

/// Logs every stdin line to `$FAKE_CLAUDE_LOG`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$FAKE_CLAUDE_LOG"
done
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
async fn cache_policy_is_shared_saved_and_a_ping_reaches_claude() {
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
    let log = tmp.path().join("received.jsonl");
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &fake);
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_LOG", &log);

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());
    let client = reqwest::Client::new();
    let start = || async {
        let res = client
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
    };
    let stop = || async {
        let res = client
            .delete(format!("{base}/agent/claude/{SID}"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
    };

    start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let snapshot = next_json(&mut ws).await;
    assert_eq!(snapshot["cachePolicy"], json!({"compactOnExpiry": false}));

    let policy = json!({"compactOnExpiry": true});
    let set = json!({"t": "cachePolicy", "policy": policy}).to_string();
    ws.send(Message::Text(set)).await.unwrap();
    let frame = next_json(&mut ws).await;
    assert_eq!(frame, json!({"t": "cachePolicy", "policy": policy}));

    let forever = json!({"t": "cachePolicy", "policy": {
        "compactOnExpiry": false, "keepWarmUntil": u64::MAX / 2,
    }});
    ws.send(Message::Text(forever.to_string())).await.unwrap();
    let frame = next_json(&mut ws).await;
    assert_eq!(frame["t"], "error", "{frame}");

    ws.send(Message::Text(json!({"t": "cachePing"}).to_string()))
        .await
        .unwrap();
    let mut received = String::new();
    for _ in 0..50 {
        received = std::fs::read_to_string(&log).unwrap_or_default();
        if received.contains(KEEPALIVE_PROMPT.split('"').next().unwrap()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let ping = received
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|l| l["type"] == "user");
    assert_eq!(
        ping.map(|l| l["message"]["content"].clone()),
        Some(json!(KEEPALIVE_PROMPT)),
        "{received}"
    );

    // The policy outlives the process: a restarted chat gets it back.
    stop().await;
    start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    assert_eq!(next_json(&mut ws).await["cachePolicy"], policy);

    stop().await;
    handle.stop().await;
}
