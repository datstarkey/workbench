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

#[tokio::test]
async fn health_sync_and_validation() {
    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

    let health = http.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(health.status(), 200);
    assert_eq!(health.text().await.unwrap(), "ok");

    // settings sync is a deliberate 501 seam.
    let sync = http.put(format!("{base}/settings/sync")).send().await.unwrap();
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
        http.get(format!("{base}/health")).send().await.unwrap().status(),
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
    let tmp = tempfile::tempdir().unwrap();
    let fake = write_fake_claude(tmp.path());
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &fake);

    let (handle, base) = start(None).await;
    let http = reqwest::Client::new();

    // Spawn in the tempdir (a valid project path).
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
