//! End-to-end tests for the headless control-plane server: a real server is
//! bound on an ephemeral port and driven over HTTP with reqwest. The
//! `claude remote-control` spawn is exercised against a fake binary (set via
//! `WORKBENCH_CLAUDE_BIN`) so no real Claude CLI / network is needed.

use std::time::Duration;

use serde_json::{json, Value};
use workbench_server::{spawn_embedded, Managers, ServerHandle};

/// Embedded listeners always require a token of at least 32 characters.
const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";

async fn start() -> (ServerHandle, String) {
    start_with(Managers::default(), TOKEN).await
}

async fn start_with(managers: Managers, token: &str) -> (ServerHandle, String) {
    let handle = spawn_embedded("127.0.0.1", 0, managers, token.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    (handle, base)
}

/// HTTP client that sends `TOKEN` on every request.
fn client() -> reqwest::Client {
    client_with(TOKEN)
}

fn client_with(token: &str) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

fn ws_url(addr: &str, id: &str) -> String {
    format!("ws://{addr}/remote/terminals/{id}/ws?token={TOKEN}")
}

#[cfg(unix)]
fn write_fake_claude(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-claude.sh");
    // Prints a session URL (so extract_url has something), then stays alive so
    // the session reports as running until killed.
    std::fs::write(
        &path,
        "#!/bin/sh\necho \"Session: https://claude.ai/code/test-abc\"\nsleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Serializes tests that mutate process-global env (WORKBENCH_CONFIG_DIR /
/// WORKBENCH_CLAUDE_BIN / WORKBENCH_MAX_*) so they don't clobber each other.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds the global env lock for the test and records every env var it sets,
/// removing them on Drop — so a panicking (failing) test can't leak a global like
/// WORKBENCH_MAX_SESSIONS=1 into sibling tests.
struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    keys: std::cell::RefCell<Vec<&'static str>>,
}

impl EnvGuard {
    fn set(&self, key: &'static str, val: impl AsRef<std::ffi::OsStr>) {
        std::env::set_var(key, val);
        self.keys.borrow_mut().push(key);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for k in self.keys.borrow().iter() {
            std::env::remove_var(k);
        }
    }
}

fn env_guard() -> EnvGuard {
    EnvGuard {
        _lock: ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner()),
        keys: std::cell::RefCell::new(Vec::new()),
    }
}

/// Point WORKBENCH_CONFIG_DIR at a fresh dir that registers `project_path` as a
/// Workbench project (so the spawn/terminal cwd allowlist accepts it). The var is
/// tracked by `env` so it's cleaned up on drop; keep the returned TempDir alive.
fn register_project(env: &EnvGuard, project_path: &std::path::Path) -> tempfile::TempDir {
    let cfg = tempfile::tempdir().unwrap();
    std::fs::write(
        cfg.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project_path }] }).to_string(),
    )
    .unwrap();
    env.set("WORKBENCH_CONFIG_DIR", cfg.path());
    cfg
}

/// `git init` a directory so `git worktree list` succeeds against it.
#[cfg(unix)]
fn git_init(dir: &std::path::Path) {
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .expect("run git init")
        .success();
    assert!(ok, "git init should succeed");
}

#[tokio::test]
async fn health_sync_and_validation() {
    let (handle, base) = start().await;
    let http = client();

    let health = http.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(health.status(), 200);
    assert_eq!(health.text().await.unwrap(), "ok");

    // settings sync is a deliberate 501 seam.
    let sync = http
        .put(format!("{base}/settings/sync"))
        .send()
        .await
        .unwrap();
    assert_eq!(sync.status(), 501);

    // empty projectPath is a client error.
    let bad = http
        .post(format!("{base}/remote/spawn"))
        .json(&json!({ "projectPath": "" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);

    handle.stop().await;
}

#[tokio::test]
async fn auth_gate() {
    let (handle, base) = start().await;
    let http = reqwest::Client::new();

    // /health is exempt.
    assert_eq!(
        http.get(format!("{base}/health"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    for path in ["/remote/sessions", "/remote/terminals", "/projects"] {
        // No token → 401.
        assert_eq!(
            http.get(format!("{base}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            401,
            "{path} without a token"
        );
        // Wrong token → 401.
        assert_eq!(
            http.get(format!("{base}{path}"))
                .bearer_auth("nope-nope-nope-nope-nope-nope-nope-nope")
                .send()
                .await
                .unwrap()
                .status(),
            401,
            "{path} with a wrong token"
        );
    }

    // Correct token → 200.
    assert_eq!(
        http.get(format!("{base}/remote/sessions"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    handle.stop().await;
}

#[tokio::test]
async fn embedded_server_refuses_a_weak_token() {
    for token in ["", "   ", "secret"] {
        let res = spawn_embedded("127.0.0.1", 0, Managers::default(), token.to_string()).await;
        assert!(res.is_err(), "token {token:?} must be refused");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_list_kill_cycle() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let fake = write_fake_claude(tmp.path());
    env.set("WORKBENCH_CLAUDE_BIN", &fake);
    // Register tmp as a Workbench project so the spawn cwd allowlist accepts it.
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let http = client();

    // Spawn in the registered project directory.
    let spawned: Value = http
        .post(format!("{base}/remote/spawn"))
        .json(&json!({ "projectPath": tmp.path(), "name": "test" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = spawned["id"].as_str().expect("id").to_string();
    assert_eq!(spawned["name"], "test");

    // Poll until the reader thread captures the URL and flips to running.
    let mut url_seen = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let sessions: Value = http
            .get(format!("{base}/remote/sessions"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let arr = sessions.as_array().unwrap();
        assert_eq!(arr.len(), 1, "exactly one tracked session");
        if arr[0]["sessionUrl"].as_str() == Some("https://claude.ai/code/test-abc") {
            assert_eq!(arr[0]["status"], "running");
            url_seen = true;
            break;
        }
    }
    assert!(url_seen, "session URL should be captured (race fix)");

    // Kill it.
    let killed = http
        .delete(format!("{base}/remote/sessions/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(killed.status(), 204);

    // Killed session is removed from the map.
    let after: Value = http
        .get(format!("{base}/remote/sessions"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after.as_array().unwrap().len(), 0);

    handle.stop().await;
    // EnvGuard drop removes WORKBENCH_CLAUDE_BIN / WORKBENCH_CONFIG_DIR.
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_rejects_unknown_worktree() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    // A real repo so list_worktrees succeeds and the known-worktree guard actually
    // runs (otherwise list_worktrees errors first and the test passes vacuously).
    git_init(tmp.path());
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let http = client();

    // Registered project + real repo, but the worktree path is not a known worktree
    // of it → rejected by the known-worktree guard.
    let res = http
        .post(format!("{base}/remote/spawn"))
        .json(&json!({
            "projectPath": tmp.path(),
            "worktreePath": "/nonexistent/worktree"
        }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_server_error());

    handle.stop().await;
}

#[tokio::test]
async fn remote_kill_is_idempotent() {
    // A delete for an unknown / already-self-exited session is a normal race, so it
    // must return 204 (idempotent) — never 500.
    let (handle, base) = start().await;
    let http = client();

    for _ in 0..2 {
        let res = http
            .delete(format!("{base}/remote/sessions/does-not-exist"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 204);
    }

    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_rejects_unregistered_dir() {
    let env = env_guard();
    // A config dir with no projects.json → zero registered projects.
    let cfg = tempfile::tempdir().unwrap();
    env.set("WORKBENCH_CONFIG_DIR", cfg.path());
    let tmp = tempfile::tempdir().unwrap();

    let (handle, base) = start().await;
    let http = client();

    // tmp exists but is not a registered Workbench project → rejected by the
    // allowlist (500 from resolve_cwd, not an earlier validation error).
    let res = http
        .post(format!("{base}/remote/spawn"))
        .json(&json!({ "projectPath": tmp.path(), "name": "x" }))
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_server_error(),
        "spawning in an unregistered directory must be rejected"
    );

    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_respects_session_cap() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let fake = write_fake_claude(tmp.path());
    env.set("WORKBENCH_CLAUDE_BIN", &fake);
    env.set("WORKBENCH_MAX_SESSIONS", "1");
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let http = client();

    let spawn = |c: &reqwest::Client| {
        c.post(format!("{base}/remote/spawn"))
            .json(&json!({ "projectPath": tmp.path() }))
            .send()
    };

    let first = spawn(&http).await.unwrap();
    assert_eq!(
        first.status(),
        200,
        "first spawn under the cap should succeed"
    );

    // The fake claude stays alive, so the slot is still taken → second hits the cap.
    let second = spawn(&http).await.unwrap();
    assert!(
        second.status().is_server_error(),
        "spawning past the session cap should be rejected"
    );

    handle.stop().await;
}

/// Create a terminal in `project` over REST and return its id.
async fn create_terminal(http: &reqwest::Client, base: &str, project: &std::path::Path) -> String {
    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({ "projectPath": project }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    meta["id"].as_str().expect("terminal id").to_string()
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_requires_token() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let id = create_terminal(&client(), &base, tmp.path()).await;

    // A browser WebSocket can't send Authorization; without ?token= the upgrade 401s.
    let no_token =
        tokio_tungstenite::connect_async(format!("ws://{addr}/remote/terminals/{id}/ws")).await;
    assert!(
        no_token.is_err(),
        "WS upgrade without a token must be rejected"
    );

    let wrong = tokio_tungstenite::connect_async(format!(
        "ws://{addr}/remote/terminals/{id}/ws?token=wrong-wrong-wrong-wrong-wrong-wrong"
    ))
    .await;
    assert!(
        wrong.is_err(),
        "WS upgrade with a wrong token must be rejected"
    );

    let with_token = tokio_tungstenite::connect_async(ws_url(&addr, &id)).await;
    assert!(
        with_token.is_ok(),
        "WS upgrade with the correct token must succeed"
    );

    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_rejects_a_foreign_origin() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Error;

    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let id = create_terminal(&client(), &base, tmp.path()).await;

    let with_origin = |origin: &str| {
        let mut req = ws_url(&addr, &id).into_client_request().unwrap();
        req.headers_mut().insert("origin", origin.parse().unwrap());
        req
    };

    // Even with the right token, a drive-by page's origin is refused.
    match tokio_tungstenite::connect_async(with_origin("https://evil.example")).await {
        Err(Error::Http(resp)) => assert_eq!(resp.status(), 403),
        other => panic!("foreign origin must get 403, got {:?}", other.map(|_| ())),
    }

    // The desktop webview's origin is allowed.
    assert!(
        tokio_tungstenite::connect_async(with_origin("tauri://localhost"))
            .await
            .is_ok(),
        "the app webview origin must be allowed"
    );

    handle.stop().await;
}

/// The desktop runs a loopback and a LAN listener over the same managers, each
/// with its own token: a terminal created through one is visible and attachable
/// through the other.
#[cfg(unix)]
#[tokio::test]
async fn listeners_sharing_managers_see_the_same_terminals() {
    const LAN_TOKEN: &str = "lan-token-abcdefabcdefabcdefabcdefabcdef";
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let managers = Managers::default();
    let (loopback, loopback_base) = start_with(managers.clone(), TOKEN).await;
    let (lan, lan_base) = start_with(managers, LAN_TOKEN).await;
    let lan_http = client_with(LAN_TOKEN);

    let id = create_terminal(&client(), &loopback_base, tmp.path()).await;

    let listed: Value = lan_http
        .get(format!("{lan_base}/remote/terminals"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == id.as_str()),
        "terminal created on loopback must be listed on the LAN listener"
    );

    let lan_addr = lan.addr().to_string();
    let attach = tokio_tungstenite::connect_async(format!(
        "ws://{lan_addr}/remote/terminals/{id}/ws?token={LAN_TOKEN}"
    ))
    .await;
    assert!(
        attach.is_ok(),
        "the LAN listener must attach to the shared terminal"
    );

    // Tokens stay per-listener.
    assert_eq!(
        client()
            .get(format!("{lan_base}/remote/terminals"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

    loopback.stop().await;
    lan.stop().await;
}

/// Stopping a listener (server mode off / token rotated) must cut off sockets
/// attached through it — they run in detached tasks that outlive the listener —
/// while the shared terminal stays alive for other listeners.
#[cfg(unix)]
#[tokio::test]
async fn stopping_a_listener_disconnects_its_attached_sockets() {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    const LAN_TOKEN: &str = "lan-token-abcdefabcdefabcdefabcdefabcdef";
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let managers = Managers::default();
    let (loopback, loopback_base) = start_with(managers.clone(), TOKEN).await;
    let (lan, _) = start_with(managers, LAN_TOKEN).await;
    let lan_addr = lan.addr().to_string();
    let id = create_terminal(&client(), &loopback_base, tmp.path()).await;

    let (mut lan_ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://{lan_addr}/remote/terminals/{id}/ws?token={LAN_TOKEN}"
    ))
    .await
    .expect("attach via the LAN listener");

    lan.stop().await;

    let outcome = tokio::time::timeout(Duration::from_secs(3), async {
        let mut saw_revoked = false;
        while let Some(Ok(msg)) = lan_ws.next().await {
            match msg {
                Message::Text(t) if t.contains(r#""t":"revoked""#) => saw_revoked = true,
                Message::Close(_) => break,
                _ => {}
            }
        }
        saw_revoked
    })
    .await;
    assert!(
        outcome.is_ok(),
        "socket must close promptly when its listener stops"
    );
    assert!(outcome.unwrap(), "socket must be told it was revoked");

    let listed: Value = client()
        .get(format!("{loopback_base}/remote/terminals"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == id.as_str() && t["alive"] == true),
        "the terminal outlives the stopped listener"
    );
    let loopback_addr = loopback.addr().to_string();
    assert!(
        tokio_tungstenite::connect_async(ws_url(&loopback_addr, &id))
            .await
            .is_ok(),
        "the terminal stays attachable through the other listener"
    );

    loopback.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_closes_when_killed() {
    use futures_util::StreamExt;
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let http = client();

    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({ "projectPath": tmp.path() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = meta["id"].as_str().expect("terminal id").to_string();

    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&addr, &id))
        .await
        .expect("WS should connect");

    // Kill the terminal; the attached socket must close rather than hang forever.
    http.delete(format!("{base}/remote/terminals/{id}"))
        .send()
        .await
        .unwrap();

    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(msg)) = ws.next().await {
            if msg.is_close() {
                break;
            }
        }
    })
    .await;
    assert!(
        ended.is_ok(),
        "attached WS must close after the terminal is killed, not hang"
    );

    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_closes_on_shell_exit() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let http = client();

    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({ "projectPath": tmp.path() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = meta["id"].as_str().expect("terminal id").to_string();

    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&addr, &id))
        .await
        .expect("WS should connect");

    // Drive the shell to exit (raw bytes → PTY). On EOF the reader signals done and
    // the attached socket must close instead of hanging on a dead shell.
    ws.send(Message::Binary(b"exit\n".to_vec())).await.unwrap();

    let ended = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(msg)) = ws.next().await {
            if msg.is_close() {
                break;
            }
        }
    })
    .await;
    assert!(
        ended.is_ok(),
        "attached WS must close after the shell exits, not hang"
    );

    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_respects_cap() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    env.set("WORKBENCH_MAX_TERMINALS", "1");
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let http = client();

    let create = |c: &reqwest::Client| {
        c.post(format!("{base}/remote/terminals"))
            .json(&json!({ "projectPath": tmp.path() }))
            .send()
    };

    let first = create(&http).await.unwrap();
    assert_eq!(
        first.status(),
        200,
        "first terminal under the cap should succeed"
    );

    let second = create(&http).await.unwrap();
    assert!(
        second.status().is_server_error(),
        "creating a terminal past the cap should be rejected"
    );

    handle.stop().await;
}

/// When a second WS client attaches to the same terminal, the FIRST client must
/// receive a `{"t":"takeover"}` text frame and then a Close, while the second
/// client continues to receive PTY output. Input from the displaced first client
/// must NOT reach the PTY (epoch guard).
#[cfg(unix)]
#[tokio::test]
async fn terminal_single_attacher_kick() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let http = client();

    // Create a terminal.
    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({ "projectPath": tmp.path() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = meta["id"].as_str().expect("terminal id").to_string();

    // Attacher A connects first.
    let (mut ws_a, _) = tokio_tungstenite::connect_async(ws_url(&addr, &id))
        .await
        .expect("WS A should connect");

    // Give A a moment to establish (epoch = 1).
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Attacher B connects — this should kick A (epoch bumps to 2).
    let (mut ws_b, _) = tokio_tungstenite::connect_async(ws_url(&addr, &id))
        .await
        .expect("WS B should connect");

    // A must receive {"t":"takeover"} and then close within 5 s.
    let takeover_and_close = tokio::time::timeout(Duration::from_secs(5), async {
        let mut saw_takeover = false;
        while let Some(Ok(msg)) = ws_a.next().await {
            match msg {
                Message::Text(t) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if v["t"] == "takeover" {
                            saw_takeover = true;
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        saw_takeover
    })
    .await;
    assert!(
        takeover_and_close.is_ok(),
        "A must receive the takeover frame and close within timeout"
    );
    assert!(
        takeover_and_close.unwrap(),
        "A must receive a {{\"t\":\"takeover\"}} text frame before closing"
    );

    // B stays open (it is the current attacher).
    // Send a unique marker via B and verify we can receive PTY output via B.
    let marker = "WORKBENCH_TEST_MARKER_B_IS_LIVE";
    ws_b.send(Message::Binary(format!("echo {marker}\n").into_bytes()))
        .await
        .unwrap();

    let b_received_output = tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(Ok(msg)) = ws_b.next().await {
            if let Message::Binary(bytes) = msg {
                if String::from_utf8_lossy(&bytes).contains(marker) {
                    return true;
                }
            }
        }
        false
    })
    .await;
    assert!(
        b_received_output.is_ok(),
        "B must still receive PTY output (not timed out)"
    );
    assert!(
        b_received_output.unwrap(),
        "B must see output echoed from PTY (A's kick must not disrupt B)"
    );

    handle.stop().await;
}

/// When the shell exits, the attached socket must receive a `{"t":"exit"}` text
/// frame (with code present or null) BEFORE (or with) the Close — strengthening
/// `terminal_ws_closes_on_shell_exit` to the new wire contract.
#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_exit_frame_carries_code() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let http = client();

    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({ "projectPath": tmp.path() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = meta["id"].as_str().expect("terminal id").to_string();

    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&addr, &id))
        .await
        .expect("WS should connect");

    // Exit the shell; the PTY reader thread will signal done_tx.
    ws.send(Message::Binary(b"exit\n".to_vec())).await.unwrap();

    // Expect to see {"t":"exit",...} and then Close (or Close containing exit),
    // all within 10 s.
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut saw_exit_frame = false;
        let mut saw_close = false;
        while let Some(Ok(msg)) = ws.next().await {
            match msg {
                Message::Text(t) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if v["t"] == "exit" {
                            saw_exit_frame = true;
                        }
                    }
                }
                Message::Close(_) => {
                    saw_close = true;
                    break;
                }
                _ => {}
            }
        }
        (saw_exit_frame, saw_close)
    })
    .await;

    assert!(
        result.is_ok(),
        "socket must close within timeout after shell exit"
    );
    let (saw_exit, saw_close) = result.unwrap();
    assert!(
        saw_exit,
        "socket must receive a {{\"t\":\"exit\"}} control frame when the shell exits"
    );
    assert!(saw_close, "socket must close after the shell exits");

    handle.stop().await;
}

/// Creating a terminal with `paneId` / `hookSocket` must forward those values
/// as `WORKBENCH_PANE_ID` / `WORKBENCH_HOOK_SOCKET` env vars into the shell.
/// We verify by spawning a shell that echoes the env var values via an initial
/// command and reading them back from the WS output stream.
#[cfg(unix)]
#[tokio::test]
async fn terminal_create_forwards_env() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let pane_id_val = "test-pane-42";
    let hook_socket_val = "/tmp/workbench-hook.sock";

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let http = client();

    // Create a terminal with paneId + hookSocket + an initial command that
    // immediately prints both env vars so we can capture them in the stream.
    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({
            "projectPath": tmp.path(),
            "paneId": pane_id_val,
            "hookSocket": hook_socket_val,
            // Print both env vars as a unique marker the test can scan for.
            "command": format!(
                "printf 'PANE_ID=%s HOOK_SOCKET=%s\\n' \"$WORKBENCH_PANE_ID\" \"$WORKBENCH_HOOK_SOCKET\""
            )
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = meta["id"].as_str().expect("terminal id").to_string();

    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&addr, &id))
        .await
        .expect("WS should connect");

    // Collect output for up to 8 s and look for the printed env values.
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut accumulated = String::new();
        while let Some(Ok(msg)) = ws.next().await {
            match msg {
                Message::Binary(bytes) => {
                    accumulated.push_str(&String::from_utf8_lossy(&bytes));
                    if accumulated.contains(&format!("PANE_ID={pane_id_val}"))
                        && accumulated.contains(&format!("HOOK_SOCKET={hook_socket_val}"))
                    {
                        return true;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        false
    })
    .await;

    // Send Ctrl-C to unstick the shell if it's waiting for more input, then exit.
    let _ = ws.send(Message::Binary(b"\x03exit\n".to_vec())).await;

    assert!(result.is_ok(), "WS output collection must not time out");
    assert!(
        result.unwrap(),
        "PTY output must contain PANE_ID and HOOK_SOCKET from env vars"
    );

    handle.stop().await;
}

/// A saved Claude account's dir becomes the shell's `CLAUDE_CONFIG_DIR`; an id
/// that names no account is refused rather than run under the default login.
#[cfg(unix)]
#[tokio::test]
async fn terminal_create_sets_claude_config_dir_for_a_saved_account() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let cfg = register_project(&env, tmp.path());
    let account_dir = tmp.path().join("claude-work");
    std::fs::write(
        cfg.path().join("settings.json"),
        json!({ "claudeAccounts": [{ "id": "work", "name": "Work", "configDir": account_dir }] })
            .to_string(),
    )
    .unwrap();

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let http = client();

    let unknown = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({ "projectPath": tmp.path(), "claudeAccountId": "nope" }))
        .send()
        .await
        .unwrap();
    assert!(
        !unknown.status().is_success(),
        "unknown account must be refused"
    );

    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .json(&json!({
            "projectPath": tmp.path(),
            "claudeAccountId": "work",
            "command": "printf 'CFG=%s\\n' \"$CLAUDE_CONFIG_DIR\""
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = meta["id"].as_str().expect("terminal id").to_string();
    assert!(
        account_dir.is_dir(),
        "config dir is created for a first login"
    );

    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(&addr, &id))
        .await
        .expect("WS should connect");
    let marker = format!("CFG={}", account_dir.display());
    let found = tokio::time::timeout(Duration::from_secs(8), async {
        let mut out = String::new();
        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Binary(bytes) = msg {
                out.push_str(&String::from_utf8_lossy(&bytes));
                if out.contains(&marker) {
                    return true;
                }
            }
        }
        false
    })
    .await;
    let _ = ws.send(Message::Binary(b"\x03exit\n".to_vec())).await;

    assert_eq!(found.ok(), Some(true), "shell must see CLAUDE_CONFIG_DIR");
    handle.stop().await;
}

/// A stand-in for `claude -p` speaking stream-json: on a prompt it streams a
/// reply and asks permission for a Bash call, then finishes the turn once the
/// host answers. Every line it receives is appended to `$FAKE_CLAUDE_LOG`.
#[cfg(unix)]
fn write_fake_stream_claude(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-claude-stream.sh");
    std::fs::write(
        &path,
        r#"#!/bin/sh
echo '{"type":"system","subtype":"init","model":"fake-model","permissionMode":"default"}'
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$FAKE_CLAUDE_LOG"
  case "$line" in
    *'/clear'*)
      echo '{"type":"conversation_reset","new_conversation_id":"2d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5","uuid":"r","session_id":"x"}'
      ;;
    *'"type":"user"'*)
      echo '{"type":"user","uuid":"u1","message":{"role":"user","content":"hello"},"parent_tool_use_id":null}'
      echo '{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1"}},"parent_tool_use_id":null}'
      echo '{"type":"assistant","uuid":"a1","message":{"id":"m1","content":[{"type":"text","text":"Hi from fake claude"}]},"parent_tool_use_id":null}'
      echo '{"type":"control_request","request_id":"perm-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}'
      ;;
    *'"request_id":"perm-1"'*)
      echo '{"type":"result","subtype":"success","is_error":false}'
      ;;
  esac
done
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[tokio::test]
async fn chat_session_streams_a_turn_and_relays_an_approval() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Error, Message};

    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());
    env.set("WORKBENCH_CLAUDE_BIN", write_fake_stream_claude(tmp.path()));
    let log = tmp.path().join("received.jsonl");
    env.set("FAKE_CLAUDE_LOG", &log);

    let (handle, base) = start().await;
    let addr = handle.addr().to_string();
    let id = "0d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5";
    let ws_url = |token: &str| format!("ws://{addr}/agent/claude/{id}/ws?token={token}");

    let res = client()
        .post(format!("{base}/agent/claude"))
        .json(&json!({ "projectPath": tmp.path(), "sessionId": id, "paneId": "pane-1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        200,
        "{}",
        res.text().await.unwrap_or_default()
    );

    // An unknown Claude account is an error, never a silent fall-back to the default login.
    let res = client()
        .post(format!("{base}/agent/claude"))
        .json(&json!({ "projectPath": tmp.path(),
            "sessionId": "1d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5", "claudeAccountId": "nope" }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_server_error() || res.status().is_client_error());

    match tokio_tungstenite::connect_async(ws_url("wrong")).await {
        Err(Error::Http(resp)) => assert_eq!(resp.status(), 401),
        other => panic!("wrong token must get 401, got {:?}", other.map(|_| ())),
    }
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url(TOKEN))
        .await
        .expect("attach");

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
    let changed_items = |frame: &Value| -> Vec<Value> {
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
    };

    assert_eq!(next_json(&mut ws).await["t"], "snapshot");
    ws.send(Message::Text(
        json!({"t":"prompt","text":"hello"}).to_string(),
    ))
    .await
    .unwrap();

    let mut saw_reply = false;
    loop {
        let frame = next_json(&mut ws).await;
        let items = changed_items(&frame);
        saw_reply |= items
            .iter()
            .any(|i| i["kind"] == "text" && i["text"] == "Hi from fake claude");
        if let Some(approval) = items.iter().find(|i| i["kind"] == "approval") {
            assert_eq!(approval["tool"], "Bash");
            break;
        }
    }
    assert!(
        saw_reply,
        "the assistant text must stream before the approval"
    );
    ws.send(Message::Text(
        json!({"t":"approve","requestId":"perm-1","decision":"allow"}).to_string(),
    ))
    .await
    .unwrap();
    loop {
        let frame = next_json(&mut ws).await;
        if frame["t"] == "update" && frame["meta"]["busy"] == false {
            break;
        }
    }

    let received = std::fs::read_to_string(&log).unwrap();
    assert!(
        received.contains(r#""subtype":"initialize""#),
        "handshake first"
    );
    assert!(received.contains(r#""origin":{"kind":"human"}"#));
    assert!(
        received.contains(r#""behavior":"allow""#),
        "approval relayed: {received}"
    );

    // /clear moves the conversation to a new id. Starting either id afterwards
    // must reach the same process — the race that once spawned a second claude.
    ws.send(Message::Text(
        json!({"t":"prompt","text":"/clear"}).to_string(),
    ))
    .await
    .unwrap();
    let new_id = "2d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5";
    loop {
        let frame = next_json(&mut ws).await;
        if frame["t"] == "snapshot" {
            assert_eq!(frame["sessionId"], new_id);
            break;
        }
    }
    for sid in [new_id, id] {
        let res = client()
            .post(format!("{base}/agent/claude"))
            .json(&json!({ "projectPath": tmp.path(), "sessionId": sid, "paneId": "pane-1" }))
            .send()
            .await
            .unwrap();
        let body: Value = res.json().await.unwrap();
        assert_eq!(
            body["sessionId"], new_id,
            "{sid} resolves to the running session"
        );
    }
    let received = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        received.matches(r#""subtype":"initialize""#).count(),
        1,
        "no second claude process was started"
    );

    let res = client()
        .delete(format!("{base}/agent/claude?paneId=pane-1"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    loop {
        if next_json(&mut ws).await["t"] == "exit" {
            break; // closing the pane stops claude
        }
    }
    // An ended session closes its socket rather than pinning it in memory.
    let closed = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap();
    assert!(
        matches!(closed, None | Some(Ok(Message::Close(_))) | Some(Err(_))),
        "socket closes after exit, got {closed:?}"
    );

    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_chat_starts_share_one_process() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());
    env.set("WORKBENCH_CLAUDE_BIN", write_fake_stream_claude(tmp.path()));
    let log = tmp.path().join("received.jsonl");
    env.set("FAKE_CLAUDE_LOG", &log);

    let (handle, base) = start().await;
    let id = "3d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5";
    let start_chat = || {
        client()
            .post(format!("{base}/agent/claude"))
            .json(&json!({ "projectPath": tmp.path(), "sessionId": id, "paneId": "pane-race" }))
            .send()
    };
    // Desktop and phone (or a remount and a reconnect) asking at once.
    let (a, b) = tokio::join!(start_chat(), start_chat());
    assert_eq!(a.unwrap().status(), 200);
    assert_eq!(b.unwrap().status(), 200);

    tokio::time::sleep(Duration::from_millis(300)).await;
    let received = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        received.matches(r#""subtype":"initialize""#).count(),
        1,
        "two starts must not spawn two claude processes"
    );

    let res = client()
        .delete(format!("{base}/agent/claude/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn chat_sessions_are_listed_and_take_messages_over_http() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());
    env.set("WORKBENCH_CLAUDE_BIN", write_fake_stream_claude(tmp.path()));
    env.set("FAKE_CLAUDE_LOG", tmp.path().join("received.jsonl"));

    let (handle, base) = start().await;
    let id = "4d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5";
    let res = client()
        .post(format!("{base}/agent/claude"))
        .json(&json!({ "projectPath": tmp.path(), "sessionId": id, "paneId": "pane-list" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);

    let res = reqwest::get(format!("{base}/agent/claude")).await.unwrap();
    assert_eq!(res.status(), 401, "listing needs the token");

    let list = || async {
        let res = client()
            .get(format!("{base}/agent/claude"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let all: Vec<Value> = res.json().await.unwrap();
        assert_eq!(all.len(), 1, "one entry per session: {all:?}");
        all.into_iter().next().unwrap()
    };
    let wait_for = |done: fn(&Value) -> bool| async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let summary = list().await;
            if done(&summary) {
                return summary;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out: {summary}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let message = |msg: Value| {
        client()
            .post(format!("{base}/agent/claude/{id}/message"))
            .body(msg.to_string())
            .send()
    };

    let summary = list().await;
    assert_eq!(summary["sessionId"], id);
    assert_eq!(summary["projectPath"], json!(tmp.path()));
    assert_eq!(summary["worktreePath"], Value::Null);
    assert_eq!(summary["paneId"], "pane-list");
    assert_eq!(summary["busy"], false);
    assert_eq!(summary["busySince"], Value::Null);
    assert_eq!(summary["waiting"], Value::Null);
    assert!(summary["updatedAt"].as_u64().unwrap() > 0);

    let res = message(json!({"t":"prompt","text":"hello"})).await.unwrap();
    assert_eq!(res.status(), 204);
    let summary = wait_for(|s| !s["waiting"].is_null()).await;
    assert_eq!(summary["waiting"]["kind"], "approval");
    assert_eq!(summary["waiting"]["id"], "perm-1");
    assert_eq!(summary["waiting"]["tool"], "Bash");
    assert_eq!(summary["busy"], true);
    assert_eq!(summary["model"], "fake-model");
    let since = summary["busySince"].as_u64().expect("busySince while busy");
    assert!(summary["updatedAt"].as_u64().unwrap() >= since);

    let res = message(json!({"t":"approve","requestId":"perm-1","decision":"allow"}))
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    let summary = wait_for(|s| s["busy"] == false).await;
    assert_eq!(summary["waiting"], Value::Null);
    assert_eq!(summary["busySince"], Value::Null);

    let res = message(json!({"t":"output","toolId":"nope"}))
        .await
        .unwrap();
    assert_eq!(res.status(), 200, "a message with a reply returns it");
    let reply: Value = res.json().await.unwrap();
    assert_eq!(reply["t"], "output");

    let res = message(json!({"t":"bogus"})).await.unwrap();
    assert_eq!(res.status(), 400);
    let res = client()
        .post(format!(
            "{base}/agent/claude/5d6f2b1e-3c4a-4b5d-8e9f-a0b1c2d3e4f5/message"
        ))
        .body(json!({"t":"interrupt"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);

    let res = client()
        .delete(format!("{base}/agent/claude/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    handle.stop().await;
}
