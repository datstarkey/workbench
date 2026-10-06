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
