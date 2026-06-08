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

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Point WORKBENCH_CONFIG_DIR at a fresh dir that registers `project_path` as a
/// Workbench project (so the spawn/terminal cwd allowlist accepts it). Keep the
/// returned TempDir alive for the duration of the test.
fn register_project(project_path: &std::path::Path) -> tempfile::TempDir {
    let cfg = tempfile::tempdir().unwrap();
    std::fs::write(
        cfg.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project_path }] }).to_string(),
    )
    .unwrap();
    std::env::set_var("WORKBENCH_CONFIG_DIR", cfg.path());
    cfg
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
    let _env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let fake = write_fake_claude(tmp.path());
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &fake);
    // Register tmp as a Workbench project so the spawn cwd allowlist accepts it.
    let _cfg = register_project(tmp.path());

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
    std::env::remove_var("WORKBENCH_CLAUDE_BIN");
    std::env::remove_var("WORKBENCH_CONFIG_DIR");
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_rejects_unknown_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

    // worktreePath that is not a known worktree of the project → error (not 200).
    let res = http
        .post(format!("{base}/remote/spawn"))
        .json(&json!({
            "projectPath": tmp.path(),
            "worktreePath": "/nonexistent/worktree"
        }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_server_error() || res.status().is_client_error());

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
    let _env = env_guard();
    // A config dir with no projects.json → zero registered projects.
    let cfg = tempfile::tempdir().unwrap();
    std::env::set_var("WORKBENCH_CONFIG_DIR", cfg.path());
    let tmp = tempfile::tempdir().unwrap();

    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

    // tmp exists but is not a registered Workbench project → must be rejected.
    let res = http
        .post(format!("{base}/remote/spawn"))
        .json(&json!({ "projectPath": tmp.path(), "name": "x" }))
        .send()
        .await
        .unwrap();
    assert!(
        res.status().is_server_error() || res.status().is_client_error(),
        "spawning in an unregistered directory must be rejected"
    );

    handle.stop().await;
    std::env::remove_var("WORKBENCH_CONFIG_DIR");
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_respects_session_cap() {
    let _env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let fake = write_fake_claude(tmp.path());
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &fake);
    std::env::set_var("WORKBENCH_MAX_SESSIONS", "1");
    let _cfg = register_project(tmp.path());

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
    std::env::remove_var("WORKBENCH_CLAUDE_BIN");
    std::env::remove_var("WORKBENCH_MAX_SESSIONS");
    std::env::remove_var("WORKBENCH_CONFIG_DIR");
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_requires_token_when_set() {
    let _env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(tmp.path());

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
    std::env::remove_var("WORKBENCH_CONFIG_DIR");
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_ws_closes_when_killed() {
    use futures_util::StreamExt;
    let _env = env_guard();
    let tmp = tempfile::tempdir().unwrap();
    let _cfg = register_project(tmp.path());

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
    std::env::remove_var("WORKBENCH_CONFIG_DIR");
}
