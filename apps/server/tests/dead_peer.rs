//! A client that vanishes without closing (a phone asleep or off the network)
//! sends no FIN, so its sockets used to stay open, each holding a descriptor,
//! until the app restarted. Accepted connections now carry TCP keepalive, and
//! terminal and chat WebSockets are pinged and dropped after two missed pongs.
//! Its own test binary because it sets process-global env.
#![cfg(unix)]

mod support;

use std::mem::ManuallyDrop;
use std::net::SocketAddr;
use std::os::fd::FromRawFd;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::json;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000002aa";
const PING_MS: u64 = 200;

const FAKE_CLAUDE: &str = r#"#!/usr/bin/env python3
import sys
for line in sys.stdin:
    pass
"#;

/// The server's end of the connection from `client`, as a descriptor of this
/// process (the test hosts the server).
fn server_end(client: SocketAddr) -> Option<ManuallyDrop<std::net::TcpStream>> {
    (0..1024).find_map(|fd| {
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1 {
            return None;
        }
        let stream = ManuallyDrop::new(unsafe { std::net::TcpStream::from_raw_fd(fd) });
        (stream.peer_addr().ok() == Some(client)).then_some(stream)
    })
}

/// Stop answering, as a vanished peer would, and expect the server to drop
/// the socket within a few pings.
async fn vanishes(url: String) {
    let (mut ws, _): (WebSocketStream<MaybeTlsStream<TcpStream>>, _) =
        tokio_tungstenite::connect_async(url).await.unwrap();
    let MaybeTlsStream::Plain(tcp) = ws.get_ref() else {
        unreachable!("plain ws")
    };
    let client = tcp.local_addr().unwrap();
    let server = server_end(client).expect("the server's end of the socket");
    let keepalive = socket2::SockRef::from(&*server).keepalive().unwrap();
    assert!(keepalive, "accepted sockets inherit TCP keepalive");

    // Not polled, the client answers no ping.
    tokio::time::sleep(Duration::from_millis(PING_MS * 5)).await;
    assert!(
        server_end(client).is_none(),
        "the server should have dropped the silent client's socket"
    );
    // What reached the client ends with the connection closing.
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(_)) = ws.next().await {}
    })
    .await
    .expect("the client should see the connection end");
}

#[tokio::test]
async fn a_client_that_vanishes_without_closing_is_dropped() {
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
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(tmp.path()));
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("WORKBENCH_WS_PING_MS", PING_MS.to_string());

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let addr = handle.addr();
    let base = format!("http://{addr}");
    let (_, id) = support::start_shell(&base, &project, None).await;
    vanishes(format!(
        "ws://{addr}/remote/terminals/{id}/ws?token={TOKEN}"
    ))
    .await;

    support::start_claude(&base, &project, SID).await;
    vanishes(format!("ws://{addr}/agent/claude/{SID}/ws?token={TOKEN}")).await;
}
