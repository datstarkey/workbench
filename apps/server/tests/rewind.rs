//! Rewinding a Claude chat against a fake `claude` (set via
//! `WORKBENCH_CLAUDE_BIN`): a dry run previews the file restore, a real one
//! restores files and restarts the process resumed before the prompt. Its
//! own test binary because it points `HOME` at a temp dir.
#![cfg(unix)]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-000000000001";
const FIRST: &str = "11111111-1111-4111-8111-111111111111";
const SECOND: &str = "22222222-2222-4222-8222-222222222222";

/// Logs its argv and checkpoint env to `$FAKE_CLAUDE_ARGS`, every stdin line
/// to `$FAKE_CLAUDE_LOG`, and answers `rewind_files` like CLI 2.1.286.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "$* checkpointing=$CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING" >> "$FAKE_CLAUDE_ARGS"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$FAKE_CLAUDE_LOG"
  id=$(printf '%s' "$line" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *'"dry_run":true'*)
      echo '{"type":"control_response","response":{"subtype":"success","request_id":"'"$id"'","response":{"canRewind":true,"filesChanged":["/w/a.txt"],"insertions":1,"deletions":2}}}'
      ;;
    *'"rewind_files"'*)
      echo '{"type":"control_response","response":{"subtype":"success","request_id":"'"$id"'","response":{"canRewind":true,"skippedLinks":0}}}'
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
        entry("assistant", "a1", Some(FIRST), text("one")),
        entry("user", SECOND, Some("a1"), json!("second")),
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
async fn rewind_previews_restores_files_and_restarts_before_the_prompt() {
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
    let args = tmp.path().join("args.log");
    let log = tmp.path().join("received.jsonl");
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &fake);
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

    let rewind = |dry_run: bool| {
        json!({"t": "rewind", "messageId": SECOND, "code": true, "conversation": true,
            "dryRun": dry_run})
        .to_string()
    };
    ws.send(Message::Text(rewind(true))).await.unwrap();
    let reply = next_json(&mut ws).await;
    assert_eq!(reply["t"], "rewind");
    assert_eq!(reply["error"], Value::Null);
    assert_eq!(reply["files"]["filesChanged"], json!(["/w/a.txt"]));

    ws.send(Message::Text(rewind(false))).await.unwrap();
    let reply = next_json(&mut ws).await;
    assert_eq!(reply["error"], Value::Null, "{reply}");
    assert_eq!(reply["files"]["canRewind"], true);
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
    assert!(launches.iter().all(|l| l.ends_with("checkpointing=true")));
    assert!(launches[1].contains(&format!("--resume {SID} --resume-session-at=a1")));
    let received = std::fs::read_to_string(&log).unwrap();
    assert!(received.contains(r#""dry_run":false"#), "{received}");

    let res = reqwest::Client::new()
        .delete(format!("{base}/agent/claude/{SID}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    handle.stop().await;
}
