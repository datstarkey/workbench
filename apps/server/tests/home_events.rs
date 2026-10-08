//! `GET /events/home`: the phone's home lists as Server-Sent Events. A
//! snapshot on connect, a fresh list when a terminal or chat comes or goes,
//! token required, and the stream ends when its listener stops. Its own test
//! binary because it points `HOME` and the config dir at a temp dir.
#![cfg(unix)]

mod support;

use std::time::Duration;

use serde_json::{json, Value};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000e7";

/// Idles until its stdin closes: the bridge makes it a chat.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
while IFS= read -r line; do :; done
"#;

struct Sse {
    res: reqwest::Response,
    buf: String,
}

impl Sse {
    async fn open(url: &str) -> Self {
        let res = reqwest::get(url).await.unwrap();
        assert_eq!(res.status(), 200);
        let kind = res.headers()["content-type"].to_str().unwrap().to_string();
        assert!(kind.starts_with("text/event-stream"), "{kind}");
        Self {
            res,
            buf: String::new(),
        }
    }

    /// The next `(event, data)`, or `None` once the stream has ended.
    async fn next(&mut self) -> Option<(String, Value)> {
        loop {
            if let Some(end) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..end + 2).collect();
                let field = |name: &str| {
                    block
                        .lines()
                        .find_map(|l| l.strip_prefix(name))
                        .map(|v| v.trim_start().to_string())
                };
                let (Some(event), Some(data)) = (field("event:"), field("data:")) else {
                    continue;
                };
                return Some((event, serde_json::from_str(&data).unwrap()));
            }
            let chunk = tokio::time::timeout(Duration::from_secs(10), self.res.chunk())
                .await
                .expect("an event within 10s")
                .expect("stream ok")?;
            self.buf.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }

    /// The next `name` event, skipping the other list.
    async fn next_of(&mut self, name: &str) -> Value {
        loop {
            let (event, data) = self.next().await.expect("stream open");
            if event == name {
                return data;
            }
        }
    }
}

#[tokio::test]
async fn the_stream_requires_the_token() {
    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .unwrap();
    let url = format!("http://{}/events/home", handle.addr());
    for bad in [url.clone(), format!("{url}?token=wrong-{TOKEN}")] {
        let res = reqwest::get(&bad).await.unwrap();
        assert_eq!(res.status(), 401, "{bad}");
    }
    let res = reqwest::Client::new()
        .get(&url)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "the header works too");
    handle.stop().await;
}

#[tokio::test]
async fn the_home_lists_stream_until_the_listener_stops() {
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
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .unwrap();
    let base = format!("http://{}", handle.addr());
    let client = reqwest::Client::new();
    let mut sse = Sse::open(&format!("{base}/events/home?token={TOKEN}")).await;

    // Both lists on connect.
    assert_eq!(sse.next().await, Some(("agents".into(), json!([]))));
    assert_eq!(sse.next().await, Some(("terminals".into(), json!([]))));

    let created: Value = client
        .post(format!("{base}/remote/terminals"))
        .bearer_auth(TOKEN)
        .json(&json!({ "projectPath": project, "name": "shell" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    let terminals = sse.next_of("terminals").await;
    assert_eq!(terminals[0]["id"], id.as_str(), "{terminals}");

    let res = client
        .delete(format!("{base}/remote/terminals/{id}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    assert_eq!(sse.next_of("terminals").await, json!([]));

    // A chat start lists the chat (and the terminal its `claude` runs in).
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
    let agents = loop {
        let agents = sse.next_of("agents").await;
        if agents.as_array().is_some_and(|a| !a.is_empty()) {
            break agents;
        }
    };
    assert_eq!(agents[0]["sessionId"], SID, "{agents}");

    let res = client
        .delete(format!("{base}/agent/claude/{SID}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    loop {
        if sse.next_of("agents").await == json!([]) {
            break;
        }
    }

    // Stopping the listener ends the stream.
    let stopped = tokio::spawn(handle.stop());
    while sse.next().await.is_some() {}
    tokio::time::timeout(Duration::from_secs(10), stopped)
        .await
        .expect("the listener stops with a stream open")
        .unwrap();
}
