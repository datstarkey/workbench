//! A macOS app started from Finder or the Dock gets launchd's soft open-file
//! limit of 256. Every terminal holds three descriptors and every socket one,
//! so a desktop with many chats and panes open (plus the phone) ran out: axum's
//! accept loop then fails with EMFILE, sleeps a second and tries again, forever,
//! and the API stops answering while existing sockets carry on. The desktop and
//! standalone server raise the soft limit at start (`watchdog::raise_fd_limit`).
//! Its own test binary because it changes the process's open-file limit.
#![cfg(unix)]

mod support;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::json;
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

/// GET `path` on a fresh connection: its status line, and how long it took.
fn probe(addr: SocketAddr, path: &str) -> (String, Duration) {
    let started = Instant::now();
    let answer = (|| {
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n"
        )?;
        let mut head = [0; 12];
        stream.read_exact(&mut head)?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&head).into_owned())
    })();
    (answer.unwrap_or_else(|e| e.to_string()), started.elapsed())
}

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

    // What the desktop does at startup, before any listener.
    workbench_server::watchdog::raise_fd_limit();
    // The probe's own end of its connection, kept for it while the load may
    // take every other descriptor: the server's end is what's under test.
    let spare = std::fs::File::open("/dev/null").unwrap();

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let addr = handle.addr();
    let base = format!("http://{addr}");

    // Each step may fail once descriptors run out; the probe says whether
    // the API still answers.
    let mut sockets = Vec::new();
    let mut chats = 0;
    for i in 0..CHATS {
        let sid = format!("5e5e5e5e-0000-4000-8000-0000000001{i:02}");
        let (_, started) = support::start_claude(&base, &project, &sid).await;
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
        let (_, id) = support::start_shell(&base, &project, None).await;
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

    drop(spare);
    // At the limit an accept can still win a descriptor something else just
    // let go of, a second or more apart: every request must be quick.
    for path in ["/health", "/agent"].repeat(3) {
        let (status, took) = tokio::task::block_in_place(|| probe(addr, path));
        assert!(
            status == "HTTP/1.1 200" && took < Duration::from_millis(500),
            "{path} answered {status:?} after {took:?}"
        );
    }
    assert_eq!((chats, terminals), (CHATS, TERMINALS));

    let agents = support::get(&base, "/agent").await;
    assert_eq!(agents.as_array().unwrap().len(), CHATS);
}
