//! Rewinding a Claude chat against a fake `claude` (set via
//! `WORKBENCH_CLAUDE_BIN`): files can't be restored from a terminal chat, and
//! a conversation rewind restarts the terminal resumed before the prompt, and
//! a `/resume` in the terminal shows the resumed session's history. Its own
//! test binary because it points `HOME` at a temp dir.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-000000000001";
const FIRST: &str = "11111111-1111-4111-8111-111111111111";
const SECOND: &str = "22222222-2222-4222-8222-222222222222";
const REPLY: &str = "33333333-3333-4333-8333-333333333333";
const OTHER: &str = "5e5e5e5e-0000-4000-8000-000000000002";

/// Logs its argv to `$FAKE_CLAUDE_ARGS` and every stdin line to
/// `$FAKE_CLAUDE_LOG`; `/resume` moves it to the other session, as the
/// plugin reports a TUI `/resume`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "$*" >> "$FAKE_CLAUDE_ARGS"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$FAKE_CLAUDE_LOG"
  case "$line" in
    *'"content":"/resume"'*)
      echo '{"type":"conversation_reset","new_conversation_id":"5e5e5e5e-0000-4000-8000-000000000002"}'
      ;;
  esac
done
"#;

fn history() -> String {
    let entry = |kind: &str, id: &str, parent: Option<&str>, content: Value| {
        let message = if kind == "user" {
            json!({"role": "user", "content": content})
        } else {
            json!({"id": format!("m-{id}"), "content": [content]})
        };
        json!({"type": kind, "uuid": id, "parentUuid": parent, "message": message}).to_string()
    };
    let text = |t: &str| json!({"type": "text", "text": t});
    [
        entry("user", FIRST, None, json!("first")),
        entry("assistant", REPLY, Some(FIRST), text("one")),
        entry("user", SECOND, Some(REPLY), json!("second")),
        entry("assistant", "a2", Some(SECOND), text("two")),
    ]
    .join("\n")
}

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

fn prompts(snapshot: &Value) -> Vec<String> {
    snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "user")
        .map(|i| i["text"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn rewind_restarts_before_the_prompt_and_says_files_stay() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-claude.sh");
    std::fs::write(&fake, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&project)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git init");
    std::fs::write(
        tmp.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project }] }).to_string(),
    )
    .unwrap();
    let sessions = tmp.path().join(".claude/projects/-project");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join(format!("{SID}.jsonl")), history()).unwrap();
    let other = json!({"type": "user", "uuid": "o1", "parentUuid": null,
        "message": {"role": "user", "content": "from the other session"}});
    std::fs::write(sessions.join(format!("{OTHER}.jsonl")), other.to_string()).unwrap();
    let args = tmp.path().join("args.log");
    let log = tmp.path().join("received.jsonl");
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_ARGS", &args);
    std::env::set_var("FAKE_CLAUDE_LOG", &log);

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());
    let start = || async {
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
    };

    start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let snapshot = next_json(&mut ws).await;
    assert_eq!(prompts(&snapshot), ["first", "second"]);

    let rewind = |code: bool, dry_run: bool| {
        json!({"t": "rewind", "messageId": SECOND, "code": code, "conversation": true,
            "dryRun": dry_run})
        .to_string()
    };
    ws.send(Message::Text(rewind(true, true))).await.unwrap();
    let reply = next_json(&mut ws).await;
    assert_eq!(reply["t"], "rewind");
    assert_eq!(reply["error"], Value::Null);
    assert_eq!(reply["files"]["canRewind"], false, "{reply}");
    assert!(reply["files"]["error"].as_str().unwrap().contains("git"));

    ws.send(Message::Text(rewind(true, false))).await.unwrap();
    let reply = next_json(&mut ws).await;
    assert!(reply["error"].as_str().is_some(), "{reply}");

    ws.send(Message::Text(rewind(false, false))).await.unwrap();
    let reply = next_json(&mut ws).await;
    assert_eq!(reply["error"], Value::Null, "{reply}");
    assert_eq!(next_json(&mut ws).await["t"], "replaced");

    // Re-attaching (as clients do on `replaced`) finds the conversation cut
    // before the prompt, under the same id.
    start().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let snapshot = next_json(&mut ws).await;
    assert_eq!(snapshot["sessionId"], SID);
    assert_eq!(prompts(&snapshot), ["first"]);

    let mut launches = String::new();
    for _ in 0..50 {
        launches = std::fs::read_to_string(&args).unwrap();
        if launches.lines().count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let launches: Vec<&str> = launches.lines().collect();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert!(launches[1].contains(&format!("--resume {SID} --resume-session-at={REPLY}")));
    let received = std::fs::read_to_string(&log).unwrap();
    assert!(!received.contains("rewind_files"), "{received}");

    ws.send(Message::Text(json!({"t": "prompt", "text": "/resume"}).to_string()))
        .await
        .unwrap();
    let snapshot = loop {
        let frame = next_json(&mut ws).await;
        if frame["t"] == "snapshot" {
            break frame;
        }
    };
    assert_eq!(snapshot["sessionId"], OTHER);
    assert_eq!(prompts(&snapshot), ["from the other session"]);

    let res = reqwest::Client::new()
        .delete(format!("{base}/agent/claude/{SID}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    handle.stop().await;
}
