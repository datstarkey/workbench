//! A terminal's plugin reports usage without the `cache_creation` split, so
//! the server learns the cache's lifetime from the session JSONL when a turn
//! ends. Its own test binary because it points `HOME` at a temp dir.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-00000000c1d1";

/// Answers a prompt as the plugin does: the JSONL row carries the split, the
/// streamed row only flat counts.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
dir="$HOME/.claude/projects/-fake"
mkdir -p "$dir"
while IFS= read -r line; do
  case "$line" in
    *'"type":"user"'*)
      printf '%s\n' '{"type":"assistant","uuid":"a1","message":{"id":"m1","usage":{"cache_creation_input_tokens":500,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":500}}}}' > "$dir/SID.jsonl"
      printf '%s\n' '{"type":"assistant","uuid":"a1","message":{"id":"m1","role":"assistant","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":500,"cache_read_input_tokens":0}}}'
      printf '%s\n' '{"type":"result","subtype":"success"}'
      ;;
  esac
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
async fn a_terminal_session_learns_the_cache_ttl_from_its_jsonl() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-claude.sh");
    std::fs::write(&fake, FAKE_CLAUDE.replace("SID", SID)).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        tmp.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project }] }).to_string(),
    )
    .unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let (pane, _) = support::start_claude(&base, &project, SID).await;

    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    next_json(&mut ws).await;
    let prompt = json!({"t": "prompt", "text": "hi"}).to_string();
    ws.send(Message::Text(prompt)).await.unwrap();

    let mut ttl = None;
    for _ in 0..20 {
        let frame = next_json(&mut ws).await;
        ttl = frame["meta"]["cacheTtlSecs"].as_u64();
        if ttl == Some(3600) {
            break;
        }
    }
    assert_eq!(ttl, Some(3600));

    support::close_pane(&base, &pane).await;
    handle.stop().await;
}
