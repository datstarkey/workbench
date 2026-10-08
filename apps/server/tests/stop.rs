//! Stopping chat sessions and listeners never holds up anything else for
//! longer than it must. Codex sessions run a fake `codex app-server` that
//! ignores the interrupt and outlives its stdin, so every stop waits out the
//! whole grace period (3s) before the kill. One process per test file: the
//! fake is set through process-global env.
#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use workbench_server::agent::{AgentManager, AgentSession, Launch, StartAgent};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const RESUMED: &str = "01a0f8c4-0000-7000-8000-000000000001";
/// The grace a stopped Codex process gets before it is killed.
const GRACE: Duration = Duration::from_secs(3);

/// Each process opens a thread of its own id; signals and stdin's end don't stop it.
const LINGERING_CODEX: &str = r#"#!/bin/sh
trap '' INT TERM HUP
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"id":\([0-9]*\),.*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      echo "{\"id\":$id,\"result\":{\"userAgent\":\"fake\"}}"
      ;;
    *'"method":"thread/start"'*)
      thread=$(printf '01a0f8c5-1c60-78a3-a1f0-%012d' $$)
      echo "{\"id\":$id,\"result\":{\"thread\":{\"id\":\"$thread\"},\"model\":\"fake-model\"}}"
      ;;
    *'"method":"thread/resume"'*)
      echo "{\"id\":$id,\"result\":{\"thread\":{\"id\":\"01a0f8c4-0000-7000-8000-000000000001\"},\"model\":\"fake-model\"}}"
      ;;
    *'"method":"'*)
      [ -n "$id" ] && echo "{\"id\":$id,\"result\":{}}"
      ;;
  esac
done
exec sleep 60
"#;

fn setup() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-codex.sh");
    std::fs::write(&fake, LINGERING_CODEX).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("WORKBENCH_CODEX_BIN", &fake);
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    tmp
}

fn codex(cwd: &std::path::Path, pane: &str, thread_id: Option<&str>) -> StartAgent {
    let cwd = cwd.to_string_lossy().into_owned();
    StartAgent {
        cwd: cwd.clone(),
        project_path: cwd,
        worktree_path: None,
        pane_id: Some(pane.into()),
        hook_socket: None,
        claude_account_id: None,
        launch: Launch::Codex {
            thread_id: thread_id.map(String::from),
            mode: None,
            options: Default::default(),
        },
    }
}

fn start(agents: &AgentManager, req: StartAgent) -> Arc<AgentSession> {
    agents.start(req).expect("codex starts")
}

#[test]
fn stopping_codex_sessions_waits_one_grace_and_holds_up_nothing_else() {
    let tmp = setup();
    let agents = AgentManager::default();

    // Three sessions of one pane stop together, not one after another.
    let sessions: Vec<_> = (0..3)
        .map(|_| start(&agents, codex(tmp.path(), "closing", None)))
        .collect();
    let stopper = {
        let agents = agents.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            assert_eq!(agents.stop_pane("closing", true), 3);
            started.elapsed()
        })
    };
    std::thread::sleep(Duration::from_millis(300));

    // Meanwhile another pane's chat starts, and the list answers.
    let started = Instant::now();
    let other = start(&agents, codex(tmp.path(), "other", None));
    assert!(
        started.elapsed() < GRACE,
        "a start waited on the stops: {:?}",
        started.elapsed()
    );
    assert_eq!(agents.summaries(None).len(), 1, "only the new one is live");

    let took = stopper.join().unwrap();
    eprintln!("3 lingering sessions stopped in {took:?}");
    assert!(
        took >= GRACE,
        "the fake lingers for the whole grace: {took:?}"
    );
    assert!(
        took < GRACE * 2,
        "stopped in parallel, not one grace each: {took:?}"
    );
    assert!(sessions.iter().all(|s| s.has_exited()));
    assert!(agents.stop(&other.id(), true));

    // A start of an id still stopping waits for its process to go: one
    // process per session file.
    let resumed = start(&agents, codex(tmp.path(), "resume", Some(RESUMED)));
    let stopper = {
        let agents = agents.clone();
        std::thread::spawn(move || agents.stop(RESUMED, false))
    };
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    let again = start(&agents, codex(tmp.path(), "resume", Some(RESUMED)));
    assert!(
        resumed.has_exited(),
        "the old process went before the new one started"
    );
    assert!(started.elapsed() >= GRACE - Duration::from_millis(500));
    assert!(!Arc::ptr_eq(&resumed, &again));
    assert!(stopper.join().unwrap());
    agents.kill_all();
}

/// A request that never finishes (here a body that never arrives) must not
/// keep a listener's stop waiting: the desktop's Settings page waits on it.
#[tokio::test]
async fn stopping_a_listener_gives_up_on_requests_that_never_finish() {
    use tokio::io::AsyncWriteExt;

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.into())
        .await
        .unwrap();
    let mut stuck = tokio::net::TcpStream::connect(handle.addr()).await.unwrap();
    let head = format!(
        "POST /agent/claude/x/message HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\n\
         Content-Type: application/json\r\nContent-Length: 1000\r\n\r\n{{",
        handle.addr()
    );
    stuck.write_all(head.as_bytes()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let started = Instant::now();
    tokio::time::timeout(Duration::from_secs(15), handle.stop())
        .await
        .expect("stop returns");
    let took = started.elapsed();
    assert!(
        took < workbench_server::STOP_DEADLINE + Duration::from_secs(1),
        "stop took {took:?}"
    );
    drop(stuck);
}
