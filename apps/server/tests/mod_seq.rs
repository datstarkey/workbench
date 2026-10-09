//! The plugin link's numbering: a `/mod/in` line stays until a later poll
//! acknowledges it (the plugin's fetch can lose a reply), and a numbered
//! `/mod/out` line folds once however often a retry or a slow post brings it.
//! Its own test binary because it sets process-global env.
#![cfg(unix)]

mod support;

use std::time::Duration;

use serde_json::{json, Value};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000ae";

/// Plays a plugin that says hello, hands the test its link, and leaves the
/// polling to the test.
const FAKE_PLUGIN: &str = r#"#!/usr/bin/env python3
import json, os, sys, time, urllib.request
args = sys.argv[1:]
sid = next(args[i + 1] for i, a in enumerate(args) if a in ("--session-id", "--resume"))
url, token = os.environ["WORKBENCH_MOD_URL"], os.environ["WORKBENCH_MOD_TOKEN"]
req = urllib.request.Request(url + "/mod/hello", data=json.dumps({"sessionId": sid}).encode(),
    method="POST", headers={"content-type": "application/json", "x-workbench-mod-token": token})
urllib.request.urlopen(req, timeout=30).read()
with open(os.environ["FAKE_CLAUDE_LINK"], "w") as f:
    json.dump({"url": url, "token": token}, f)
time.sleep(600)
"#;

#[tokio::test]
async fn link_lines_are_delivered_until_acknowledged_and_folded_once() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-plugin.py");
    std::fs::write(&fake, FAKE_PLUGIN).unwrap();
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
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &fake);
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_LINK", &link_file);

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let client = reqwest::Client::new();
    let (pane, _) = support::start_claude(&base, &project, SID).await;
    let mut link = Value::Null;
    for _ in 0..50 {
        if let Ok(read) =
            serde_json::from_str(&std::fs::read_to_string(&link_file).unwrap_or_default())
        {
            link = read;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mod_url = link["url"].as_str().unwrap().to_string();
    let mod_token = link["token"].as_str().unwrap().to_string();
    let poll = |ack: u64| {
        client
            .get(format!("{mod_url}/mod/in?sessionId={SID}&ack={ack}"))
            .header("x-workbench-mod-token", &mod_token)
            .send()
    };
    let prompt = |text: &str| {
        client
            .post(format!("{base}/agent/claude/{SID}/message"))
            .bearer_auth(TOKEN)
            .json(&json!({"t": "prompt", "text": text}))
            .send()
    };
    let seqs = |lines: &Value| -> Vec<u64> {
        lines
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["wbSeq"].as_u64().unwrap())
            .collect()
    };

    // The attach queued the SDK's `initialize`; a prompt follows it.
    assert!(prompt("one").await.unwrap().status().is_success());
    let first: Value = poll(0).await.unwrap().json().await.unwrap();
    assert_eq!(seqs(&first), [1, 2]);
    assert_eq!(first[1]["type"], "user");
    // That reply was lost: the next poll acknowledges nothing and gets them again.
    let again: Value = poll(0).await.unwrap().json().await.unwrap();
    assert_eq!(again, first);
    assert!(prompt("two").await.unwrap().status().is_success());
    let next: Value = poll(2).await.unwrap().json().await.unwrap();
    assert_eq!(seqs(&next), [3]);

    let hello = |epoch: &str| {
        client
            .post(format!("{mod_url}/mod/hello"))
            .header("x-workbench-mod-token", &mod_token)
            .json(&json!({"sessionId": SID, "epoch": epoch}))
            .send()
    };
    // Attaching again (a lost hello reply, a restarted worker) loads nothing and
    // counts from the newest line acknowledged, so 3 comes again.
    let again: Value = hello("w1").await.unwrap().json().await.unwrap();
    assert_eq!(again, json!({"inSeq": 2, "loaded": false}));

    let out = |epoch: &str, seq: u64, titles: &[&str]| {
        let lines: Vec<Value> = titles
            .iter()
            .map(|t| json!({"type": "custom-title", "customTitle": t}))
            .collect();
        client
            .post(format!("{mod_url}/mod/out"))
            .header("x-workbench-mod-token", &mod_token)
            .json(&json!({"sessionId": SID, "epoch": epoch, "seq": seq, "lines": lines}))
            .send()
    };
    let title = || async {
        let agents: Value = client
            .get(format!("{base}/agent"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        agents[0]["title"].as_str().unwrap_or_default().to_string()
    };
    let posted = |res: reqwest::Result<reqwest::Response>| res.unwrap().status().is_success();
    assert!(posted(out("w1", 1, &["First"]).await));
    assert_eq!(title().await, "First");
    // A retry resends line 1 with line 2; a hung post of line 1 lands last.
    assert!(posted(out("w1", 1, &["Stale", "Second"]).await));
    assert_eq!(title().await, "Second");
    assert!(posted(out("w1", 1, &["Stale"]).await));
    assert_eq!(title().await, "Second");

    // The worker restarted: it numbers from 1 again, and the old one's late
    // post is dropped.
    assert!(hello("w2").await.unwrap().status().is_success());
    assert!(posted(out("w2", 1, &["Third"]).await));
    assert_eq!(title().await, "Third");
    assert!(posted(out("w1", 3, &["Old"]).await));
    assert_eq!(title().await, "Third");

    support::close_pane(&base, &pane).await;
    handle.stop().await;
}
