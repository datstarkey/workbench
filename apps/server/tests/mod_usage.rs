//! What a terminal `claude`'s plugin reports reaches the rest of the server:
//! a `rate_limit_event`'s windows answer `GET /agent/usage` for the session's
//! account (no `claude -p /usage` run), and a chat's model pick reaches it
//! resolved to the id its requests name. Its own test binary because it sets
//! process-global env.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::SinkExt;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000ab";

/// Reports plan usage at start, answers `initialize` with a model list, and
/// logs every stdin line to `$FAKE_CLAUDE_LOG`.
const FAKE_CLAUDE: &str = r#"#!/usr/bin/env python3
import json, os, sys

def out(value):
    print(json.dumps(value), flush=True)

out({"type": "rate_limit_event",
     "rate_limit_info": {"status": "allowed", "rateLimitType": "seven_day", "utilization": 0.4},
     "windows": [{"kind": "five_hour", "percentUsed": 12.5, "resetsAt": 4102444800},
                 {"kind": "seven_day", "percentUsed": 40, "resetsAt": 4102444800}]})
with open(os.environ["FAKE_CLAUDE_LOG"], "a") as log:
    for line in sys.stdin:
        log.write(line)
        log.flush()
        msg = json.loads(line)
        if msg.get("request", {}).get("subtype") == "initialize":
            out({"type": "control_response", "response": {
                "subtype": "success", "request_id": msg["request_id"], "response": {
                    "models": [{"value": "sonnet", "resolvedModel": "claude-sonnet-5-5"}]}}})
"#;

#[tokio::test]
async fn plugin_usage_answers_plan_usage_and_a_model_pick_is_resolved() {
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
    let log = tmp.path().join("received.jsonl");
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_LOG", &log);

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let client = reqwest::Client::new();
    let (pane, _) = support::start_claude(&base, &project, SID).await;

    let mut usage = Value::Null;
    for _ in 0..50 {
        usage = client
            .get(format!("{base}/agent/usage"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if usage[0]["label"] == "session" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        usage,
        json!([
            { "label": "session", "percent": 13, "resetsAt": 4102444800u64 },
            { "label": "week (all models)", "percent": 40, "resetsAt": 4102444800u64 }
        ])
    );

    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let pick = json!({"t": "model", "model": "sonnet"}).to_string();
    let mut set_model = None;
    for _ in 0..50 {
        // The list comes with the `initialize` reply; a pick before it can't resolve.
        ws.send(Message::Text(pick.clone())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        set_model = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .map(|l| l["request"].clone())
            .find(|r| r["subtype"] == "set_model" && !r["resolvedModel"].is_null());
        if set_model.is_some() {
            break;
        }
    }
    assert_eq!(
        set_model,
        Some(json!({"subtype": "set_model", "model": "sonnet",
                "resolvedModel": "claude-sonnet-5-5", "effortLevels": []}))
    );

    support::close_pane(&base, &pane).await;
    handle.stop().await;
}
