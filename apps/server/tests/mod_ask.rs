//! A question the plugin asks in chat (`/mod/ask`) survives a lost reply: the
//! host's fetch has been seen never to settle, so the plugin asks again, and
//! must get the same answer back without the card reopening. Its own test
//! binary because it sets process-global env.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000ac";

/// Hands the test the terminal's mod link, then idles.
const FAKE_CLAUDE: &str = r#"#!/usr/bin/env python3
import json, os, sys
with open(os.environ["FAKE_CLAUDE_LINK"], "w") as f:
    json.dump({"url": os.environ["WORKBENCH_MOD_URL"], "token": os.environ["WORKBENCH_MOD_TOKEN"]}, f)
for line in sys.stdin:
    pass
"#;

#[tokio::test]
async fn an_answer_whose_reply_was_lost_is_answered_again() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-claude.py");
    std::fs::write(&fake, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        tmp.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project }] }).to_string(),
    )
    .unwrap();
    let link_file = tmp.path().join("link.json");
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_LINK", &link_file);

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let client = reqwest::Client::new();
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

    let mut link = Value::Null;
    for _ in 0..50 {
        let text = std::fs::read_to_string(&link_file).unwrap_or_default();
        if let Ok(read) = serde_json::from_str(&text) {
            link = read;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mod_url = link["url"].as_str().unwrap().to_string();
    let mod_token = link["token"].as_str().unwrap().to_string();
    let question = "Which one?";
    let line = json!({
        "type": "control_request",
        "request_id": "ask-1",
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "AskUserQuestion",
            "tool_use_id": "toolu_1",
            "input": {"questions": [{"question": question, "header": "Pick",
                "multiSelect": false, "options": [{"label": "A"}, {"label": "B"}]}]}
        }
    });
    let ask = |line: Option<&Value>| {
        let mut body = json!({"sessionId": SID, "requestId": "ask-1", "hold": true});
        if let Some(line) = line {
            body["line"] = line.clone();
        }
        client
            .post(format!("{mod_url}/mod/ask"))
            .header("x-workbench-mod-token", &mod_token)
            .json(&body)
            .send()
    };

    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let first = tokio::spawn(ask(Some(&line)));
    // The card reaches the chat; answer it there.
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("the question should reach the chat")
            .unwrap()
            .unwrap();
        if frame.to_text().unwrap().contains("ask-1") {
            break;
        }
    }
    let answer = json!({"t": "approve", "requestId": "ask-1", "decision": "allow",
        "answers": {question: "B"}});
    ws.send(Message::Text(answer.to_string())).await.unwrap();
    let reply: Value = first.await.unwrap().unwrap().json().await.unwrap();
    let answered = &reply["answer"]["response"]["response"];
    assert_eq!(answered["updatedInput"]["answers"], json!({question: "B"}));

    // The plugin never saw that reply: it asks again, resending the line.
    let again: Value = tokio::time::timeout(Duration::from_secs(5), ask(Some(&line)))
        .await
        .expect("the kept answer comes back at once")
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(&again["answer"]["response"]["response"], answered);
    let agents: Value = client
        .get(format!("{base}/agent/claude"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        agents[0]["waiting"].is_null(),
        "the resend reopened the card: {agents}"
    );

    // Its call has a result: the answer is spent, and with no chat open the
    // terminal would ask a repeat itself.
    let result = json!({"type": "user", "message": {"role": "user",
        "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "B"}]}});
    let res = client
        .post(format!("{mod_url}/mod/out"))
        .header("x-workbench-mod-token", &mod_token)
        .json(&json!({"sessionId": SID, "lines": [result]}))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    ws.close(None).await.unwrap();
    drop(ws);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let spent: Value = ask(None).await.unwrap().json().await.unwrap();
    assert_eq!(spent, json!({"fallback": true}));

    // Asked with no chat open: the terminal's dialog takes it, and the session
    // waits on it there until its call has a result.
    let mut second = line.clone();
    second["request_id"] = json!("ask-2");
    second["request"]["tool_use_id"] = json!("toolu_2");
    let fell: Value = client
        .post(format!("{mod_url}/mod/ask"))
        .header("x-workbench-mod-token", &mod_token)
        .json(&json!({"sessionId": SID, "requestId": "ask-2", "line": second}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fell, json!({"fallback": true}));
    let waiting = || async {
        let agents: Value = client
            .get(format!("{base}/agent/claude"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        agents[0]["waiting"].clone()
    };
    let w = waiting().await;
    assert_eq!(
        (&w["id"], &w["inTerminal"]),
        (&json!("ask-2"), &json!(true)),
        "{w}"
    );
    let result = json!({"type": "user", "message": {"role": "user",
        "content": [{"type": "tool_result", "tool_use_id": "toolu_2", "content": "B"}]}});
    client
        .post(format!("{mod_url}/mod/out"))
        .header("x-workbench-mod-token", &mod_token)
        .json(&json!({"sessionId": SID, "lines": [result]}))
        .send()
        .await
        .unwrap();
    assert!(waiting().await.is_null());

    let res = client
        .delete(format!("{base}/agent/claude/{SID}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    handle.stop().await;
}
