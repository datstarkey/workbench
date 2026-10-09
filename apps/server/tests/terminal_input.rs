//! A paste bigger than the PTY's input queue, into a shell that isn't reading,
//! must not stall the server: input is written on the session's own thread and
//! the socket is pushed back on, never a tokio worker blocked on the PTY.
//! Its own test binary because it runs the server on a one-worker runtime.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const PASTE: usize = 512 * 1024;
const CHUNK: usize = 4 * 1024;

async fn wait_for(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    marker: &str,
    within: Duration,
) {
    let mut seen = String::new();
    tokio::time::timeout(within, async {
        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Binary(bytes) = msg {
                seen.push_str(&String::from_utf8_lossy(&bytes));
                if seen.contains(marker) {
                    return;
                }
            }
        }
        panic!("socket closed before {marker}");
    })
    .await
    .unwrap_or_else(|_| panic!("no {marker} within {within:?}"));
}

// The server gets a runtime of its own with one worker: before the fix, the
// attach task's blocking PTY write took that worker, and nothing else on the
// server (here, `/health`) ran until the shell read.
#[test]
fn a_paste_into_a_busy_shell_neither_stalls_the_server_nor_loses_input() {
    let server = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let handle = server
        .block_on(spawn_embedded(
            "127.0.0.1",
            0,
            Managers::default(),
            TOKEN.to_string(),
        ))
        .unwrap();
    let addr = handle.addr().to_string();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(paste_and_check(&addr));
    server.block_on(handle.stop());
}

async fn paste_and_check(addr: &str) {
    let project = tempfile::tempdir().unwrap();
    let cfg = tempfile::tempdir().unwrap();
    std::fs::write(
        cfg.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project.path() }] }).to_string(),
    )
    .unwrap();
    std::env::set_var("WORKBENCH_CONFIG_DIR", cfg.path());

    let http = reqwest::Client::new();

    // Raw mode, so the paste isn't cut into canonical lines or echoed; the
    // markers are printed with `%s` so the echoed command line can't match them.
    let command = format!(
        "stty raw -echo; printf 'RE%sDY' A; sleep 5; head -c {PASTE} > pasted; printf 'DO%sE' N"
    );
    let base = format!("http://{addr}");
    let (_, id) = support::start_shell(&base, project.path(), Some(&command)).await;

    let (ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://{addr}/remote/terminals/{id}/ws?token={TOKEN}"
    ))
    .await
    .unwrap();
    let (mut sink, mut stream) = ws.split();
    wait_for(&mut stream, "READY", Duration::from_secs(10)).await;

    let payload: Vec<u8> = (0..PASTE).map(|i| b'a' + (i % 26) as u8).collect();
    let paste = {
        let payload = payload.clone();
        tokio::spawn(async move {
            for chunk in payload.chunks(CHUNK) {
                sink.send(Message::Binary(chunk.to_vec())).await.unwrap();
            }
            sink
        })
    };

    // The shell is asleep with its input queue full; the server still answers.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let health = tokio::time::timeout(
        Duration::from_secs(2),
        http.get(format!("http://{addr}/health")).send(),
    )
    .await
    .expect("/health answers while a paste is pending")
    .unwrap();
    assert!(health.status().is_success());

    wait_for(&mut stream, "DONE", Duration::from_secs(20)).await;
    let _sink = paste.await.unwrap();
    let pasted = std::fs::read(project.path().join("pasted")).unwrap();
    assert_eq!(pasted.len(), payload.len(), "every pasted byte arrives");
    assert!(pasted == payload, "and in order");
}
