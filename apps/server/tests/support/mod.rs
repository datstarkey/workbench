//! A Claude chat runs as interactive `claude` in a server terminal, made a
//! chat by the Workbench plugin over `/mod/*`. Tests can't run the real CLI,
//! so `WORKBENCH_CLAUDE_BIN` points at this bridge: it plays the plugin
//! (hello, post stdout, poll stdin) around a stand-in `claude -p` that speaks
//! stream-json on stdio, named by `WORKBENCH_FAKE_CLAUDE`.

#![allow(dead_code)]

const BRIDGE: &str = r#"#!/usr/bin/env python3
import json, os, subprocess, sys, threading, time, urllib.request

url = os.environ["WORKBENCH_MOD_URL"]
token = os.environ["WORKBENCH_MOD_TOKEN"]
args = sys.argv[1:]
sid = next(args[i + 1] for i, a in enumerate(args) if a in ("--session-id", "--resume"))


def call(method, path, body=None, timeout=30):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url + path, data=data, method=method, headers={
        "content-type": "application/json", "x-workbench-mod-token": token})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        text = resp.read()
        return json.loads(text) if text else None


fake = subprocess.Popen([os.environ["WORKBENCH_FAKE_CLAUDE"], *args],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
call("POST", "/mod/hello", {"sessionId": sid})


def pump():
    for line in fake.stdout:
        try:
            value = json.loads(line)
        except ValueError:
            continue
        call("POST", "/mod/out", {"sessionId": sid, "lines": [value]})


threading.Thread(target=pump, daemon=True).start()
while fake.poll() is None:
    try:
        lines = call("GET", "/mod/in?sessionId=" + sid) or []
    except Exception:
        time.sleep(0.2)
        continue
    for line in lines:
        fake.stdin.write(json.dumps(line, separators=(",", ":")) + "\n")
        fake.stdin.flush()
"#;

/// Write the bridge into `dir`; point `WORKBENCH_CLAUDE_BIN` at the returned
/// path and `WORKBENCH_FAKE_CLAUDE` at the stand-in.
#[cfg(unix)]
pub fn mod_bridge(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("claude-mod-bridge.py");
    std::fs::write(&path, BRIDGE).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

/// The bearer token every embedded test server runs with.
pub const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";

pub async fn get(base: &str, path: &str) -> Value {
    let res = reqwest::Client::new()
        .get(format!("{base}{path}"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "GET {path}");
    res.json().await.unwrap()
}

/// `POST /workspace/commands`, which must be accepted.
pub async fn command(base: &str, cmd: Value) -> Value {
    let res = reqwest::Client::new()
        .post(format!("{base}/workspace/commands"))
        .bearer_auth(TOKEN)
        .json(&cmd)
        .send()
        .await
        .unwrap();
    let status = res.status();
    let body: Value = res.json().await.unwrap();
    assert_eq!(status, 200, "{cmd} → {body}");
    body
}

/// A `newSession` in `project`'s workspace; `extra` adds fields (`resume`,
/// `accountId`, `codexMode`, `command`, `prompt`, `worktreePath`). Its pane id.
pub async fn new_session(base: &str, project: &Path, kind: &str, extra: Value) -> String {
    let mut cmd = json!({ "type": "newSession", "projectPath": project, "kind": kind });
    if let Value::Object(fields) = extra {
        cmd.as_object_mut().unwrap().extend(fields);
    }
    command(base, cmd).await["paneId"]
        .as_str()
        .unwrap()
        .to_string()
}

pub async fn close_pane(base: &str, pane: &str) {
    command(base, json!({ "type": "closePane", "paneId": pane })).await;
}

/// Poll `GET /agent` until `id` is listed (as its id or a previous one).
pub async fn wait_for_agent(base: &str, id: &str) -> Value {
    for _ in 0..300 {
        let list = get(base, "/agent").await;
        let found = list.as_array().unwrap().iter().find(|s| {
            s["sessionId"] == id
                || s["previousIds"]
                    .as_array()
                    .is_some_and(|p| p.contains(&json!(id)))
        });
        if let Some(summary) = found {
            return summary.clone();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("session {id} never listed");
}

/// Poll `GET /agent` until session `id` is no longer listed.
pub async fn wait_agent_gone(base: &str, id: &str) {
    for _ in 0..300 {
        let list = get(base, "/agent").await;
        if !list
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["sessionId"] == id)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("session {id} still listed");
}

/// A Claude chat of session `sid` in `project`, once its plugin attached:
/// its pane and its `GET /agent` summary (`terminalId` is its terminal).
pub async fn start_claude(base: &str, project: &Path, sid: &str) -> (String, Value) {
    let pane = new_session(base, project, "claude", json!({ "resume": sid })).await;
    (pane, wait_for_agent(base, sid).await)
}

/// The pane as the workspace snapshots show it, once `pred` accepts it.
pub async fn wait_for_pane(
    base: &str,
    pane: &str,
    what: &str,
    pred: impl Fn(&Value) -> bool,
) -> Value {
    let mut sse = Sse::open(base).await;
    let snap = sse
        .until(what, |s| find_pane(s, pane).is_some_and(&pred))
        .await;
    find_pane(&snap, pane).unwrap().clone()
}

/// A `newSession` the host refuses, as a command or at its pane's spawn: the error.
pub async fn refused_session(base: &str, project: &Path, kind: &str, extra: Value) -> String {
    let mut cmd = json!({ "type": "newSession", "projectPath": project, "kind": kind });
    if let Value::Object(fields) = extra {
        cmd.as_object_mut().unwrap().extend(fields);
    }
    let res = reqwest::Client::new()
        .post(format!("{base}/workspace/commands"))
        .bearer_auth(TOKEN)
        .json(&cmd)
        .send()
        .await
        .unwrap();
    let ok = res.status().is_success();
    let body: Value = res.json().await.unwrap();
    if !ok {
        return body["error"].as_str().unwrap().to_string();
    }
    let pane = body["paneId"].as_str().unwrap();
    let p = wait_for_pane(base, pane, "the refusal", |p| p["error"].is_string()).await;
    assert_eq!(p["status"], "exited", "{p}");
    close_pane(base, pane).await;
    p["error"].as_str().unwrap().to_string()
}

/// A shell pane typing `command`: its pane and terminal ids.
pub async fn start_shell(base: &str, project: &Path, command: Option<&str>) -> (String, String) {
    let extra = command.map_or(json!({}), |c| json!({ "command": c }));
    let pane = new_session(base, project, "shell", extra).await;
    let p = wait_for_pane(base, &pane, "a terminal", |p| {
        p["terminalId"].is_string() || p["status"] == "exited"
    })
    .await;
    let terminal = p["terminalId"]
        .as_str()
        .unwrap_or_else(|| panic!("the shell failed: {p}"));
    (pane, terminal.to_string())
}

/// Poll until terminal `id` is no longer listed alive.
pub async fn wait_terminal_gone(base: &str, id: &str) {
    for _ in 0..100 {
        let list = get(base, "/remote/terminals").await;
        if !list
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == id && t["alive"] == true)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("terminal {id} still alive");
}

pub fn find_pane<'a>(snap: &'a Value, id: &str) -> Option<&'a Value> {
    snap["workspaces"]
        .as_array()?
        .iter()
        .flat_map(|w| w["tabs"].as_array().into_iter().flatten())
        .flat_map(|t| t["panes"].as_array().into_iter().flatten())
        .find(|p| p["id"] == id)
}

/// `GET /events/workspace`, read snapshot by snapshot.
pub struct Sse {
    res: reqwest::Response,
    buf: String,
}

impl Sse {
    pub async fn open(base: &str) -> Self {
        let res = reqwest::get(format!("{base}/events/workspace?token={TOKEN}"))
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        Self {
            res,
            buf: String::new(),
        }
    }

    /// The next snapshot, or `None` once the stream has ended.
    pub async fn next(&mut self) -> Option<Value> {
        loop {
            if let Some(end) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..end + 2).collect();
                let field = |name: &str| {
                    block
                        .lines()
                        .find_map(|l| l.strip_prefix(name))
                        .map(|v| v.trim_start().to_string())
                };
                if field("event:").as_deref() != Some("snapshot") {
                    continue;
                }
                return Some(serde_json::from_str(&field("data:")?).unwrap());
            }
            let chunk = tokio::time::timeout(Duration::from_secs(20), self.res.chunk())
                .await
                .expect("an event within 20s")
                .ok()??;
            self.buf.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }

    /// The first snapshot `pred` accepts.
    pub async fn until(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        loop {
            let snap = self
                .next()
                .await
                .unwrap_or_else(|| panic!("ended waiting for {what}"));
            if pred(&snap) {
                return snap;
            }
        }
    }
}
