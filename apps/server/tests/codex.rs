//! Codex chat sessions against a fake `codex app-server` (set via
//! `WORKBENCH_CODEX_BIN`) that answers the JSON-RPC handshake, plays one turn
//! with a command approval, and serves history on resume. One test, since it
//! sets process-global env.
#![cfg(unix)]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const NEW_THREAD: &str = "01a0f8c5-1c60-78a3-a1f0-a30542fec38b";
const OLD_THREAD: &str = "01a0f8c4-0000-7000-8000-000000000001";

/// Requests arrive as `{"id":N,"jsonrpc":"2.0","method":…}` (serde_json sorts
/// keys); every line received is appended to `$FAKE_CODEX_LOG`.
const FAKE_CODEX: &str = r#"#!/bin/sh
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$FAKE_CODEX_LOG"
  id=$(printf '%s' "$line" | sed -n 's/^{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"decision"'*)
      echo '{"method":"serverRequest/resolved","params":{"requestId":0}}'
      echo '{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"exec-1","command":"/bin/zsh -lc ls","cwd":"/w","status":"completed","aggregatedOutput":"a.txt","exitCode":0}}}'
      echo '{"method":"turn/completed","params":{"turn":{"id":"turn-1","status":"completed","error":null}}}'
      ;;
    *'"method":"initialize"'*)
      echo "{\"id\":$id,\"result\":{\"userAgent\":\"fake\"}}"
      ;;
    *'"method":"thread/start"'*)
      echo "{\"id\":$id,\"result\":{\"thread\":{\"id\":\"01a0f8c5-1c60-78a3-a1f0-a30542fec38b\",\"name\":null,\"preview\":\"\"},\"model\":\"fake-model\",\"approvalPolicy\":\"on-request\",\"sandbox\":{\"type\":\"readOnly\",\"networkAccess\":false}}}"
      ;;
    *'"method":"thread/resume"'*)
      echo "{\"id\":$id,\"result\":{\"thread\":{\"id\":\"01a0f8c4-0000-7000-8000-000000000001\",\"name\":\"Old chat\"},\"model\":\"fake-model\",\"approvalPolicy\":\"never\",\"sandbox\":{\"type\":\"dangerFullAccess\"},\"itemsBackwardsCursor\":\"c1\"}}"
      ;;
    *'"method":"thread/items/list"'*)
      echo "{\"id\":$id,\"result\":{\"data\":[{\"item\":{\"type\":\"agentMessage\",\"id\":\"m0\",\"text\":\"earlier answer\"}},{\"item\":{\"type\":\"userMessage\",\"id\":\"u0\",\"content\":[{\"type\":\"text\",\"text\":\"earlier question\"}]}}],\"nextCursor\":null}}"
      ;;
    *'"method":"thread/compact/start"'*)
      echo "{\"id\":$id,\"result\":{}}"
      ;;
    *'"method":"thread/fork"'*)
      echo "{\"id\":$id,\"result\":{\"thread\":{\"id\":\"01a0f8c4-0000-7000-8000-000000000002\",\"name\":\"Fork\"}}}"
      ;;
    *'"method":"remoteControl/enable"'*)
      echo "{\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"Unsupported\"}}"
      ;;
    *'"method":"thread/unsubscribe"'*)
      echo "{\"id\":$id,\"result\":{\"status\":\"unsubscribed\"}}"
      ;;
    *'"method":"model/list"'*)
      echo "{\"id\":$id,\"result\":{\"data\":[{\"id\":\"fake-model\",\"model\":\"fake-model\",\"displayName\":\"Fake\",\"description\":\"\",\"hidden\":false,\"supportedReasoningEfforts\":[{\"reasoningEffort\":\"low\"},{\"reasoningEffort\":\"high\"}]}]}}"
      ;;
    *'"method":"turn/start"'*)
      echo "{\"id\":$id,\"result\":{\"turn\":{\"id\":\"turn-1\",\"status\":\"inProgress\"}}}"
      echo '{"method":"turn/started","params":{"turn":{"id":"turn-1"}}}'
      echo '{"method":"item/completed","params":{"item":{"type":"userMessage","id":"u1","content":[{"type":"text","text":"hello"}]}}}'
      echo '{"method":"item/started","params":{"item":{"type":"agentMessage","id":"m1","text":""}}}'
      echo '{"method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"Hi from fake codex"}}'
      echo '{"method":"item/started","params":{"item":{"type":"commandExecution","id":"exec-1","command":"/bin/zsh -lc ls","cwd":"/w","status":"inProgress"}}}'
      echo '{"method":"item/commandExecution/requestApproval","id":0,"params":{"itemId":"exec-1","command":"/bin/zsh -lc ls","cwd":"/w","reason":"May I list?"}}'
      ;;
  esac
done
"#;

fn client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {TOKEN}").parse().unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

async fn next_json(ws: &mut (impl StreamExt<Item = Result<Message, Error>> + Unpin)) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("frame within 5s")
            .expect("stream open")
            .expect("frame ok");
        if let Message::Text(text) = frame {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

fn changed_items(frame: &Value) -> Vec<Value> {
    match frame["t"].as_str() {
        Some("update") => frame["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c[1].clone())
            .collect(),
        Some("snapshot") => frame["items"].as_array().unwrap().clone(),
        _ => vec![],
    }
}

#[tokio::test]
async fn codex_chat_starts_streams_approves_resumes_and_stops() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-codex.sh");
    std::fs::write(&fake, FAKE_CODEX).unwrap();
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
    // Registered, and with the sandbox runtime on: srt never wraps Codex, so
    // Codex chat (unlike Claude's) still starts.
    std::fs::write(
        tmp.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project }] }).to_string(),
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("settings.json"),
        json!({ "sandboxRuntimeEnabled": true, "codexApprovalPolicy": "on-request", "codexSandboxMode": "read-only" }).to_string(),
    )
    .unwrap();
    let log = tmp.path().join("received.jsonl");
    std::env::set_var("WORKBENCH_CODEX_BIN", &fake);
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CODEX_LOG", &log);

    let managers = Managers::default();
    let desktop_feed = managers.agents.attention.clone();
    let handle = spawn_embedded("127.0.0.1", 0, managers, TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let ws_url = |id: &str| format!("ws://{}/agent/codex/{id}/ws?token={TOKEN}", handle.addr());
    let start = |body: Value| {
        client()
            .post(format!("{base}/agent/codex"))
            .json(&body)
            .send()
    };
    let list = |path: &'static str| {
        let url = format!("{base}{path}");
        async move {
            let response = client().get(url).send().await.unwrap();
            assert!(
                response.headers().contains_key(reqwest::header::DATE),
                "Android uses the server's Date to catch a newly listed first-turn completion"
            );
            response.json::<Vec<Value>>().await.unwrap()
        }
    };

    let initial: Value = client()
        .get(format!("{base}/agent/attention"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(initial["events"], json!([]));
    let initial_cursor = initial["cursor"].as_str().unwrap().to_string();
    assert_eq!(initial_cursor, desktop_feed.since(None).cursor);
    assert_eq!(
        reqwest::get(format!("{base}/agent/attention"))
            .await
            .unwrap()
            .status(),
        401
    );

    // A new thread: the start answers with codex's thread id.
    let res = start(json!({ "projectPath": project, "paneId": "pane-1" }))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "{}",
        res.text().await.unwrap_or_default()
    );
    assert_eq!(res.json::<Value>().await.unwrap()["sessionId"], NEW_THREAD);

    let all = list("/agent").await;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["agent"], "codex");
    assert_eq!(all[0]["sessionId"], NEW_THREAD);
    assert_eq!(all[0]["paneId"], "pane-1");
    assert_eq!(all[0]["turnEndedAt"], Value::Null);
    assert!(
        list("/agent/claude").await.is_empty(),
        "older phones see Claude only"
    );
    assert_eq!(list("/agent/codex").await.len(), 1);

    let res =
        start(json!({ "projectPath": "/nowhere", "sessionId": OLD_THREAD, "attachOnly": true }))
            .await
            .unwrap();
    assert_eq!(res.status(), 404, "attach-only never spawns");
    let res =
        start(json!({ "projectPath": "/nowhere", "sessionId": NEW_THREAD, "attachOnly": true }))
            .await
            .unwrap();
    assert_eq!(res.status(), 200);
    let res = start(json!({ "projectPath": project, "codexMode": "yolo" }))
        .await
        .unwrap();
    assert!(!res.status().is_success(), "an unknown mode is refused");

    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(NEW_THREAD))
        .await
        .expect("attach");
    let snapshot = next_json(&mut ws).await;
    assert_eq!(snapshot["t"], "snapshot");
    assert_eq!(snapshot["sessionId"], NEW_THREAD);
    assert_eq!(snapshot["commands"], json!([]));
    assert_eq!(snapshot["meta"]["model"], "fake-model");
    assert_eq!(snapshot["meta"]["permissionMode"], "read-only");
    assert_eq!(
        snapshot["meta"]["models"][0]["effortLevels"],
        json!(["low", "high"])
    );

    ws.send(Message::Text(
        json!({"t":"effort","effort":"high"}).to_string(),
    ))
    .await
    .unwrap();
    assert_eq!(next_json(&mut ws).await["meta"]["effort"], "high");
    // A malformed upload fails before starting a turn.
    let invalid = client()
        .post(format!("{base}/agent/codex/{NEW_THREAD}/message"))
        .json(&json!({"t":"prompt", "text":"bad upload", "files":[
            {"name":"bad.pdf", "mediaType":"application/pdf", "data":"A"}
        ]}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 400);
    assert_eq!(list("/agent").await[0]["busy"], false);

    // Both hosts send this same payload: file uploads become tool-readable
    // references, while an image stays native app-server input.
    ws.send(Message::Text(
        json!({"t":"prompt","text":"hello", "images":[
            {"mediaType":"image/png", "data":"aGk="}
        ], "files":[
            {"name":"notes with spaces.rs", "mediaType":"text/plain", "data":"fn main() {} // ü"},
            {"name":"../../../report with spaces.pdf", "mediaType":"application/pdf", "data":"JVBERg=="}
        ]}).to_string(),
    ))
    .await
    .unwrap();
    let mut saw_reply = false;
    let approval = loop {
        let frame = next_json(&mut ws).await;
        let items = changed_items(&frame);
        saw_reply |= items
            .iter()
            .any(|i| i["kind"] == "text" && i["text"] == "Hi from fake codex");
        if let Some(a) = items.into_iter().find(|i| i["kind"] == "approval") {
            break a;
        }
    };
    assert!(saw_reply, "the reply streams before the approval");
    assert_eq!(approval["tool"], "Bash");
    assert_eq!(approval["input"]["command"], "ls");
    assert_eq!(approval["description"], "May I list?");
    let waiting = &list("/agent").await[0]["waiting"];
    assert_eq!(waiting["preview"], "ls");
    assert_eq!(waiting["id"], approval["id"]);

    let waiting_events: Value = client()
        .get(format!("{base}/agent/attention"))
        .query(&[("cursor", &initial_cursor)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(waiting_events["events"][0]["kind"], "waiting");
    assert_eq!(waiting_events["events"][0]["sessionId"], NEW_THREAD);
    assert_eq!(waiting_events["events"][0]["waiting"]["id"], approval["id"]);
    assert_eq!(
        waiting_events,
        serde_json::to_value(desktop_feed.since(Some(&initial_cursor))).unwrap(),
        "desktop and phone consume the same plugin/app-server-driven event batch"
    );
    let waiting_cursor = waiting_events["cursor"].as_str().unwrap().to_string();

    let approve = json!({"t":"approve","requestId": approval["id"],"decision":"allow"});
    ws.send(Message::Text(approve.to_string())).await.unwrap();
    let mut saw_output = false;
    loop {
        let frame = next_json(&mut ws).await;
        saw_output |= changed_items(&frame)
            .iter()
            .any(|i| i["kind"] == "tool" && i["output"] == "a.txt" && i["status"] == "ok");
        if frame["t"] == "update" && frame["meta"]["busy"] == false {
            break;
        }
    }
    assert!(saw_output, "the command finishes with its output");
    let completed = &list("/agent").await[0];
    assert_eq!(completed["busy"], false);
    assert_eq!(completed["waiting"], Value::Null);
    assert!(
        completed["turnEndedAt"].as_u64().is_some(),
        "Codex completion is visible to Android polling even after its busy frame was missed"
    );

    let completed_events: Value = client()
        .get(format!("{base}/agent/attention"))
        .query(&[("cursor", &waiting_cursor)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let kinds: Vec<&str> = completed_events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["resolved", "turnEnded"]);
    assert_eq!(
        completed_events,
        serde_json::to_value(desktop_feed.since(Some(&waiting_cursor))).unwrap()
    );
    assert!(
        desktop_feed
            .since(completed_events["cursor"].as_str())
            .events
            .is_empty(),
        "catch-up doesn't repeat an alert"
    );
    // Explicit native actions are correlated on the socket; forking keeps this
    // session's identity, and unavailable optional APIs report an action error.
    for (request, action) in [
        ("compact", "compact"),
        ("fork", "fork"),
        ("remote", "remoteEnable"),
    ] {
        ws.send(Message::Text(
            json!({"t":"codex","requestId":request,"action":action}).to_string(),
        ))
        .await
        .unwrap();
        loop {
            let frame = next_json(&mut ws).await;
            if frame["t"] != "codexResult" || frame["requestId"] != request {
                continue;
            }
            if action == "remoteEnable" {
                assert_eq!(frame["error"], "Unsupported");
            } else if action == "fork" {
                assert_eq!(frame["result"]["thread"]["name"], "Fork");
            } else {
                assert!(frame.get("error").is_none());
            }
            break;
        }
    }
    assert_eq!(list("/agent/codex").await[0]["sessionId"], NEW_THREAD);

    let received = std::fs::read_to_string(&log).unwrap();
    let sent: Vec<Value> = received
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let by_method = |m: &str| sent.iter().find(|v| v["method"] == m).cloned().unwrap();
    assert_eq!(sent[0]["method"], "initialize");
    assert_eq!(sent[0]["params"]["clientInfo"]["name"], "workbench");
    assert_eq!(sent[1]["method"], "initialized");
    let thread = by_method("thread/start");
    assert_eq!(thread["params"]["approvalPolicy"], "on-request");
    assert_eq!(thread["params"]["sandbox"], "read-only");
    let turn = by_method("turn/start");
    assert_eq!(turn["params"]["threadId"], NEW_THREAD);
    let input = turn["params"]["input"].as_array().unwrap();
    assert_eq!(input.len(), 2);
    assert_eq!(input[0]["type"], "text");
    assert_eq!(input[0]["text_elements"], json!([]));
    let prompt = input[0]["text"].as_str().unwrap();
    assert!(prompt.starts_with("hello\n\n@"));
    assert!(prompt.contains("Read them with your tools"));
    let mentions = prompt.split("\n\n").nth(1).unwrap();
    let paths: Vec<std::path::PathBuf> = mentions
        .split('"')
        .skip(1)
        .step_by(2)
        .map(Into::into)
        .collect();
    assert_eq!(paths.len(), 2);
    let attachment_dir = std::env::temp_dir().join("workbench-chat").join(NEW_THREAD);
    assert!(paths.iter().all(|p| p.starts_with(&attachment_dir)));
    assert_eq!(
        std::fs::read_to_string(&paths[0]).unwrap(),
        "fn main() {} // ü"
    );
    assert_eq!(std::fs::read(&paths[1]).unwrap(), b"%PDF");
    assert_eq!(paths[1].file_name().unwrap(), "2-report with spaces.pdf");
    assert_eq!(
        input[1],
        json!({"type":"image", "url":"data:image/png;base64,aGk="})
    );
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(turn["params"]["effort"], "high");
    assert!(
        sent.iter()
            .any(|v| v["id"] == 0 && v["result"]["decision"] == "accept"),
        "approval relayed: {received}"
    );

    // Resume: history arrives in the snapshot, oldest first; a second start
    // for the same thread joins the running process.
    for _ in 0..2 {
        let res =
            start(json!({ "projectPath": project, "sessionId": OLD_THREAD, "paneId": "pane-2", "codexMode": "full-access" }))
                .await
                .unwrap();
        assert_eq!(
            res.status(),
            200,
            "{}",
            res.text().await.unwrap_or_default()
        );
        assert_eq!(res.json::<Value>().await.unwrap()["sessionId"], OLD_THREAD);
    }
    let received = std::fs::read_to_string(&log).unwrap();
    assert_eq!(received.matches(r#""method":"thread/resume""#).count(), 1);
    let resume: Value = received
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["method"] == "thread/resume")
        .unwrap();
    assert_eq!(
        resume["params"]["approvalPolicy"], "never",
        "an explicit mode overrides the saved read-only default"
    );
    assert_eq!(resume["params"]["sandbox"], "danger-full-access");
    let (mut old_ws, _) = tokio_tungstenite::connect_async(ws_url(OLD_THREAD))
        .await
        .unwrap();
    let snapshot = next_json(&mut old_ws).await;
    let texts: Vec<&str> = snapshot["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["earlier question", "earlier answer"]);
    assert_eq!(snapshot["meta"]["title"], "Old chat");
    assert_eq!(snapshot["meta"]["permissionMode"], "full-access");
    assert_eq!(list("/agent/codex").await.len(), 2);

    // Closing a pane stops its chat whatever the kind, via either route.
    let res = client()
        .delete(format!("{base}/agent/claude?paneId=pane-1"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    loop {
        let frame = next_json(&mut ws).await;
        if frame["t"] == "exit" {
            // Only an End tells other viewers to close the chat.
            assert_eq!(frame["ended"], false);
            break;
        }
    }
    let res = client()
        .delete(format!("{base}/agent/codex/{OLD_THREAD}?end=true"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    loop {
        let frame = next_json(&mut old_ws).await;
        if frame["t"] == "exit" {
            assert_eq!(frame["ended"], true);
            break;
        }
    }
    assert!(list("/agent").await.is_empty());
    assert!(
        !attachment_dir.exists(),
        "stopping Codex removes its uploads"
    );

    // A broken CLI closes stdout while leaving stdin unread. A large queued
    // write must not deadlock cleanup or keep its socket alive indefinitely.
    let stalled = r#"#!/bin/sh
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) echo "{\"id\":$id,\"result\":{}}" ;;
    *'"method":"thread/start"'*)
      echo "{\"id\":$id,\"result\":{\"thread\":{\"id\":\"01a0f8c5-1c60-78a3-a1f0-a30542fec38b\"}}}"
      trap '' TERM
      sleep 2
      exec 1>&-
      sleep 30
      ;;
  esac
done
"#;
    std::fs::write(&fake, stalled).unwrap();
    let res = start(json!({"projectPath":project})).await.unwrap();
    assert_eq!(
        res.status(),
        200,
        "{}",
        res.text().await.unwrap_or_default()
    );
    let (mut stalled_ws, _) = tokio_tungstenite::connect_async(ws_url(NEW_THREAD))
        .await
        .unwrap();
    assert_eq!(next_json(&mut stalled_ws).await["t"], "snapshot");
    stalled_ws
        .send(Message::Text(
            json!({"t":"prompt","text":"x".repeat(2 * 1024 * 1024)}).to_string(),
        ))
        .await
        .unwrap();
    loop {
        if next_json(&mut stalled_ws).await["t"] == "exit" {
            break;
        }
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        while !list("/agent").await.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("stdout EOF releases a stalled writer and removes the session");

    handle.stop().await;
}
