//! End-to-end tests for the headless control-plane server: a real server is
//! bound on an ephemeral port and driven over HTTP with reqwest. The
//! `claude remote-control` spawn is exercised against a fake binary (set via
//! `WORKBENCH_CLAUDE_BIN`) so no real Claude CLI / network is needed.

use std::time::Duration;

use serde_json::{json, Value};
use workbench_server::{spawn_embedded, ServerHandle};

async fn start(token: Option<&str>) -> (ServerHandle, String) {
    let handle = spawn_embedded("127.0.0.1", 0, token.map(|t| t.to_string()))
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    (handle, base)
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
    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

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
    let (handle, base) = start(Some("secret")).await;
    let http = reqwest::Client::new();

    // /health is exempt even when a token is configured.
    assert_eq!(
        http.get(format!("{base}/health"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    // protected route without a token → 401.
    assert_eq!(
        http.get(format!("{base}/remote/sessions"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

    // wrong token → 401.
    assert_eq!(
        http.get(format!("{base}/remote/sessions"))
            .bearer_auth("nope")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

    // correct token → 200.
    assert_eq!(
        http.get(format!("{base}/remote/sessions"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .status(),
        200
    );

    handle.stop().await;
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

    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

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

    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

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
    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

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

    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

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

    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

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

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_requires_token_when_set() {
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start(Some("secret")).await;
    let addr = handle.addr().to_string();
    let http = reqwest::Client::new();

    // Create a terminal over the header-authed REST route.
    let meta: Value = http
        .post(format!("{base}/remote/terminals"))
        .bearer_auth("secret")
        .json(&json!({ "projectPath": tmp.path() }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = meta["id"].as_str().expect("terminal id").to_string();

    // A browser WebSocket can't send Authorization; without ?token= the upgrade 401s.
    let no_token =
        tokio_tungstenite::connect_async(format!("ws://{addr}/remote/terminals/{id}/ws")).await;
    assert!(
        no_token.is_err(),
        "WS upgrade without a token must be rejected when a token is configured"
    );

    // With the correct ?token= the upgrade succeeds.
    let with_token = tokio_tungstenite::connect_async(format!(
        "ws://{addr}/remote/terminals/{id}/ws?token=secret"
    ))
    .await;
    assert!(
        with_token.is_ok(),
        "WS upgrade with the correct token must succeed"
    );

    handle.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_closes_when_killed() {
    use futures_util::StreamExt;
    let env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(&env, tmp.path());

    let (handle, base) = start(None).await;
    let addr = handle.addr().to_string();
    let http = reqwest::Client::new();

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

    let (mut ws, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/remote/terminals/{id}/ws"))
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

    let (handle, base) = start(None).await;
    let addr = handle.addr().to_string();
    let http = reqwest::Client::new();

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

    let (mut ws, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/remote/terminals/{id}/ws"))
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

    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

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
