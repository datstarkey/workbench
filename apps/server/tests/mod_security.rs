//! What a terminal's token lets its holder do over `/mod/*`. Anything in a
//! chat's terminal holds it (a prompt-injected Bash too), so it must not reach
//! another session: no reset onto another live chat's id, no attach under an
//! id it wasn't given, and no `/mod/*` on a LAN listener at all. Its own test
//! binary because it sets process-global env.
#![cfg(unix)]

mod support;

use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID_A: &str = "5e5e5e5e-0000-4000-8000-00000000000a";
const SID_B: &str = "5e5e5e5e-0000-4000-8000-00000000000b";
const SID_C: &str = "5e5e5e5e-0000-4000-8000-00000000000c";
const SID_D: &str = "5e5e5e5e-0000-4000-8000-00000000000d";

/// Hands the test its terminal's mod link and env, then idles.
const FAKE_CLAUDE: &str = r#"#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
sid = next(args[i + 1] for i, a in enumerate(args) if a in ("--session-id", "--resume"))
keys = ["WORKBENCH_MOD_URL", "WORKBENCH_MOD_TOKEN", "WORKBENCH_PANE_ID",
        "WORKBENCH_HOOK_SOCKET", "CLAUDE_CODE_PLUGIN_DIRS"]
with open(os.path.join(os.environ["FAKE_CLAUDE_LINKS"], sid + ".json"), "w") as f:
    json.dump({k: os.environ.get(k) for k in keys}, f)
for line in sys.stdin:
    pass
"#;

async fn read_link(dir: &Path, sid: &str) -> Value {
    let file = dir.join(format!("{sid}.json"));
    for _ in 0..100 {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if let Ok(read) = serde_json::from_str(&text) {
            return read;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{sid}'s terminal never started");
}

#[tokio::test]
async fn a_terminal_token_reaches_only_its_own_session() {
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
    let links = tmp.path().join("links");
    std::fs::create_dir(&links).unwrap();
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_LINKS", &links);
    // What a parent Workbench leaves in the env of one started from its terminal.
    std::env::set_var("WORKBENCH_PANE_ID", "parent-pane");
    std::env::set_var("WORKBENCH_HOOK_SOCKET", "/parent/hook.sock");
    let parent_plugin = "/parent/.workbench/claude-plugin/workbench";
    std::env::set_var(
        "CLAUDE_CODE_PLUGIN_DIRS",
        std::env::join_paths([parent_plugin, "/mine"]).unwrap(),
    );

    let managers = Managers::default();
    let handle = spawn_embedded("127.0.0.1", 0, managers.clone(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let lan = spawn_embedded("127.0.0.1", 0, managers, TOKEN.to_string())
        .await
        .expect("second listener should bind");
    let base = format!("http://{}", handle.addr());
    let client = reqwest::Client::new();
    for sid in [SID_A, SID_B] {
        let res = client
            .post(format!("{base}/agent/claude"))
            .bearer_auth(TOKEN)
            .json(&json!({ "projectPath": project, "sessionId": sid }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            200,
            "{}",
            res.text().await.unwrap_or_default()
        );
    }

    let link = read_link(&links, SID_A).await;
    let ours = tmp.path().join("claude-plugin").join("workbench");
    let plugin_dirs: Vec<_> =
        std::env::split_paths(link["CLAUDE_CODE_PLUGIN_DIRS"].as_str().unwrap()).collect();
    assert_eq!(
        plugin_dirs,
        [ours, "/mine".into()],
        "ours first, the parent's copy gone"
    );
    assert_eq!(link["WORKBENCH_PANE_ID"], Value::Null, "the parent's pane");
    assert_eq!(
        link["WORKBENCH_HOOK_SOCKET"],
        Value::Null,
        "the parent's bridge"
    );

    let mod_url = link["WORKBENCH_MOD_URL"].as_str().unwrap().to_string();
    let mod_token = link["WORKBENCH_MOD_TOKEN"].as_str().unwrap().to_string();
    let post = |url: String, path: &str, body: Value| {
        client
            .post(format!("{url}{path}"))
            .header("x-workbench-mod-token", &mod_token)
            .json(&body)
            .send()
    };
    let out = |lines: Value| {
        post(
            mod_url.clone(),
            "/mod/out",
            json!({"sessionId": SID_A, "lines": lines}),
        )
    };
    let summaries = || async {
        let list: Value = client
            .get(format!("{base}/agent/claude"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        list.as_array().unwrap().clone()
    };
    let summary = |list: &[Value], sid: &str| {
        list.iter()
            .find(|s| s["sessionId"] == sid)
            .cloned()
            .unwrap_or_else(|| panic!("{sid} isn't listed: {list:?}"))
    };

    // An approval whose asking hook died goes with its call's result, or the turn.
    let ask = |id: &str, tool: &str| {
        json!({"type": "control_request", "request_id": id, "request": {
            "subtype": "can_use_tool", "tool_name": "Bash", "tool_use_id": tool,
            "input": {"command": "ls"}}})
    };
    out(json!([ask("r1", "toolu_1")])).await.unwrap();
    assert_eq!(summary(&summaries().await, SID_A)["waiting"]["id"], "r1");
    let result = json!({"type": "user", "message": {"role": "user",
        "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}]}});
    out(json!([result])).await.unwrap();
    assert!(summary(&summaries().await, SID_A)["waiting"].is_null());
    out(json!([ask("r2", "toolu_2")])).await.unwrap();
    assert_eq!(summary(&summaries().await, SID_A)["waiting"]["id"], "r2");
    out(json!([{"type": "result", "subtype": "success"}]))
        .await
        .unwrap();
    assert!(summary(&summaries().await, SID_A)["waiting"].is_null());

    // A `/clear` onto a fresh id moves A there, and the token may attach as it.
    let line = json!({"type": "conversation_reset", "new_conversation_id": SID_C});
    out(json!([line])).await.unwrap();
    assert_eq!(
        summary(&summaries().await, SID_C)["previousIds"],
        json!([SID_A])
    );
    for (sid, ok) in [(SID_C, true), (SID_B, false), (SID_D, false)] {
        let res = post(mod_url.clone(), "/mod/hello", json!({"sessionId": sid}))
            .await
            .unwrap();
        assert_eq!(res.status().is_success(), ok, "attach as {sid}");
    }

    // The LAN listener doesn't serve `/mod/*` at all.
    let lan_url = format!("http://{}", lan.addr());
    let res = post(lan_url, "/mod/hello", json!({"sessionId": SID_C}))
        .await
        .unwrap();
    assert_eq!(res.status(), 404);

    // A reset naming B's id leaves B alone, and this chat lets go of its terminal.
    let ws_url = format!(
        "ws://{}/agent/claude/{SID_C}/ws?token={TOKEN}",
        handle.addr()
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let line = json!({"type": "conversation_reset", "new_conversation_id": SID_B});
    out(json!([line])).await.unwrap();
    let mut frames = String::new();
    while let Ok(Some(Ok(frame))) = tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
        frames.push_str(frame.to_text().unwrap_or_default());
        if frames.contains("another chat already has open") {
            break;
        }
    }
    assert!(frames.contains("another chat already has open"), "{frames}");
    let list = summaries().await;
    assert_eq!(summary(&list, SID_B)["previousIds"], json!([]));
    assert!(
        list.iter().all(|s| s["sessionId"] != SID_C),
        "detached: {list:?}"
    );
    let url = format!(
        "ws://{}/agent/claude/{SID_B}/ws?token={TOKEN}",
        handle.addr()
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let snapshot: Value =
        serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(snapshot["sessionId"], SID_B, "B's clients still reach B");

    // A malformed id is refused the same way.
    let b = read_link(&links, SID_B).await;
    let res = client
        .post(format!("{mod_url}/mod/out"))
        .header(
            "x-workbench-mod-token",
            b["WORKBENCH_MOD_TOKEN"].as_str().unwrap(),
        )
        .json(&json!({"sessionId": SID_B, "lines": [
            {"type": "conversation_reset", "new_conversation_id": "../../etc"}]}))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    assert!(summaries()
        .await
        .iter()
        .all(|s| s["sessionId"] != "../../etc"));

    // Both chats let go of their terminals, which keep polling until killed.
    let terminals: Value = client
        .get(format!("{base}/remote/terminals"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for terminal in terminals.as_array().unwrap() {
        let id = terminal["id"].as_str().unwrap();
        client
            .delete(format!("{base}/remote/terminals/{id}"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap();
    }
    lan.stop().await;
    handle.stop().await;
}
