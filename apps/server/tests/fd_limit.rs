//! A macOS app started from Finder or the Dock gets launchd's soft open-file
//! limit of 256. Every terminal holds three descriptors and every socket one,
//! so a desktop with many chats and panes open (plus the phone) ran out: axum's
//! accept loop then fails with EMFILE, sleeps a second and tries again, forever,
//! and the API stops answering while existing sockets carry on. The server
//! raises the soft limit to what the hard limit allows when it starts.
//! Its own test binary because it changes the process's open-file limit.
#![cfg(unix)]

mod support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const LAUNCHD_SOFT_LIMIT: libc::rlim_t = 256;
const CHATS: usize = 8;
const TERMINALS: usize = 40;

/// Asks for an approval at once, then idles: the chat holds a waiting card.
const FAKE_CLAUDE: &str = r#"#!/usr/bin/env python3
import json, sys
print(json.dumps({"type": "control_request", "request_id": "ask-1", "request": {
    "subtype": "can_use_tool", "tool_name": "Bash", "tool_use_id": "toolu_1",
    "input": {"command": "ls"}}}), flush=True)
for line in sys.stdin:
    pass
"#;

/// Started before the load, since the test process may have no descriptors
/// left to spawn one later: on each line (a URL), GETs it and prints
/// `<status> <seconds>`.
const PROBE: &str = r#"
import sys, time, urllib.request
token = sys.argv[1]
for url in sys.stdin:
    start = time.monotonic()
    try:
        req = urllib.request.Request(url.strip(), headers={"authorization": "Bearer " + token})
        status = urllib.request.urlopen(req, timeout=3).status
    except Exception as e:
        status = type(e).__name__
    print(status, round(time.monotonic() - start, 3), flush=True)
"#;

/// Counted without opening one (`/dev/fd` can't be read once they run out).
fn open_fds() -> usize {
    (0..1024)
        .filter(|&fd| unsafe { libc::fcntl(fd, libc::F_GETFD) } != -1)
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_api_answers_with_many_sessions_open_under_launchds_file_limit() {
    use std::os::unix::fs::PermissionsExt;
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    limit.rlim_cur = LAUNCHD_SOFT_LIMIT;
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);

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
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());

    let mut probe = Command::new("python3")
        .args(["-c", PROBE, TOKEN])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut probe_in = probe.stdin.take().unwrap();
    let mut probe_out = BufReader::new(probe.stdout.take().unwrap());

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let addr = handle.addr();
    let base = format!("http://{addr}");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // Each step may fail once descriptors run out; the probe says whether
    // the API still answers.
    let mut sockets = Vec::new();
    let mut chats = 0;
    for i in 0..CHATS {
        let sid = format!("5e5e5e5e-0000-4000-8000-0000000001{i:02}");
        let Ok(res) = http
            .post(format!("{base}/agent/claude"))
            .bearer_auth(TOKEN)
            .json(&json!({ "projectPath": project, "sessionId": sid }))
            .send()
            .await
        else {
            break;
        };
        let Ok(started) = res.json::<Value>().await else {
            break;
        };
        let Some(terminal) = started["terminalId"].as_str() else {
            break;
        };
        let urls = [
            format!("ws://{addr}/agent/claude/{sid}/ws?token={TOKEN}"),
            format!("ws://{addr}/remote/terminals/{terminal}/ws?token={TOKEN}"),
        ];
        for url in urls {
            match tokio_tungstenite::connect_async(url).await {
                Ok((ws, _)) => sockets.push(ws),
                Err(_) => break,
            }
        }
        chats += 1;
    }
    let mut terminals = 0;
    for _ in 0..TERMINALS {
        let Ok(res) = http
            .post(format!("{base}/remote/terminals"))
            .bearer_auth(TOKEN)
            .json(&json!({ "projectPath": project }))
            .send()
            .await
        else {
            break;
        };
        let Some(id) = res
            .json::<Value>()
            .await
            .ok()
            .and_then(|m| m["id"].as_str().map(String::from))
        else {
            break;
        };
        match tokio_tungstenite::connect_async(format!(
            "ws://{addr}/remote/terminals/{id}/ws?token={TOKEN}"
        ))
        .await
        {
            Ok((ws, _)) => sockets.push(ws),
            Err(_) => break,
        }
        terminals += 1;
    }
    eprintln!(
        "{chats} chats, {terminals} terminals, {} sockets; {} descriptors open",
        sockets.len(),
        open_fds()
    );

    for path in ["/health", "/agent/claude"] {
        writeln!(probe_in, "{base}{path}").unwrap();
        let mut line = String::new();
        tokio::task::block_in_place(|| probe_out.read_line(&mut line)).unwrap();
        let (status, secs) = line.trim().split_once(' ').unwrap();
        let secs: f64 = secs.parse().unwrap();
        assert!(
            status == "200" && secs < 1.0,
            "{path} answered {status} after {secs}s"
        );
    }
    assert_eq!((chats, terminals), (CHATS, TERMINALS));

    let agents: Vec<Value> = http
        .get(format!("{base}/agent/claude"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(agents.len(), CHATS);
    drop(probe_in);
    let _ = probe.wait();
}
