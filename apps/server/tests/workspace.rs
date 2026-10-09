//! The workspace service end to end: commands over HTTP, snapshots over SSE,
//! real terminals, and Claude through the plugin bridge (`support`). Its own
//! binary because it points `HOME`, the config dir and the CLIs at a temp dir;
//! the tests share that env and run one at a time.
#![cfg(unix)]

mod support;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};
use support::{command, find_pane as pane, Sse};
use tokio_tungstenite::tungstenite::Message;
use workbench_server::workspace::WorkspaceService;
use workbench_server::{spawn_embedded, Managers, ServerHandle};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";

/// Idles until its stdin closes: the bridge makes it a chat.
const FAKE_CLAUDE: &str = "#!/bin/sh\nwhile IFS= read -r line; do :; done\n";
/// Prints how it was launched, then idles like a TUI.
const FAKE_CODEX: &str =
    "#!/bin/sh\n[ \"$1\" = --help ] && exit 0\necho \"FAKECODEX $*\"\nexec cat\n";

struct Env {
    dir: PathBuf,
    project: PathBuf,
}

fn env() -> &'static Env {
    static ENV: OnceLock<Env> = OnceLock::new();
    ENV.get_or_init(|| {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap().keep();
        let script = |name: &str, body: &str| {
            let path = dir.join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let project = dir.join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(
            dir.join("projects.json"),
            json!({ "projects": [{ "name": "test", "path": project }] }).to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join("settings.json"),
            json!({ "codexApprovalPolicy": "never", "codexSandboxMode": "read-only" }).to_string(),
        )
        .unwrap();
        std::env::set_var("HOME", &dir);
        std::env::set_var("WORKBENCH_CONFIG_DIR", &dir);
        std::env::set_var(
            "WORKBENCH_FAKE_CLAUDE",
            script("fake-claude.sh", FAKE_CLAUDE),
        );
        std::env::set_var("WORKBENCH_CLAUDE_BIN", support::mod_bridge(&dir));
        std::env::set_var("WORKBENCH_CODEX_BIN", script("fake-codex.sh", FAKE_CODEX));
        Env { dir, project }
    })
}

/// One test at a time: they share the env and the config dir.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let guard = LOCK.lock().await;
    env();
    guard
}

async fn serve(managers: Managers) -> (ServerHandle, String) {
    let handle = spawn_embedded("127.0.0.1", 0, managers, TOKEN.to_string())
        .await
        .unwrap();
    let base = format!("http://{}", handle.addr());
    (handle, base)
}

fn new_session(kind: &str) -> Value {
    json!({ "type": "newSession", "projectPath": env().project, "kind": kind })
}

fn running(snap: &Value, id: &str) -> bool {
    pane(snap, id).is_some_and(|p| p["status"] == "running" && p["terminalId"].is_string())
}

/// Read a terminal's output until it contains `needle`.
async fn terminal_shows(base: &str, terminal: &str, needle: &str) -> String {
    let url = format!(
        "{}/remote/terminals/{terminal}/ws?token={TOKEN}",
        base.replace("http://", "ws://")
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let mut out = String::new();
    let found = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Binary(bytes) = msg {
                out.push_str(&String::from_utf8_lossy(&bytes));
                if out.contains(needle) {
                    return;
                }
            }
        }
    })
    .await;
    assert!(found.is_ok(), "{needle:?} not in {out:?}");
    out
}

async fn alive_terminals(base: &str) -> Vec<String> {
    let list: Value = reqwest::Client::new()
        .get(format!("{base}/remote/terminals"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .filter(|t| t["alive"] == true)
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_new_session_shows_its_pane_then_runs() {
    let _serial = serial().await;
    let (handle, base) = serve(Managers::default()).await;
    let mut sse = Sse::open(&base).await;
    let reply = command(&base, new_session("claude")).await;
    let id = reply["paneId"].as_str().unwrap().to_string();

    let shown = sse.until("the pane", |s| pane(s, &id).is_some()).await;
    assert!(shown["rev"].as_u64().unwrap() <= reply["rev"].as_u64().unwrap());
    assert_eq!(pane(&shown, &id).unwrap()["kind"], "claude");
    let ran = sse.until("running", |s| running(s, &id)).await;
    let ran = pane(&ran, &id).unwrap();
    assert!(ran["sessionId"].is_string());
    // The activity Home shows comes with the pane.
    for field in ["busySince", "turnEndedAt", "running", "waitingSince"] {
        assert!(ran.get(field).is_some(), "{field} in {ran}");
    }
    handle.stop().await;
}

#[tokio::test]
async fn closing_a_pane_ends_its_process_everywhere() {
    let _serial = serial().await;
    let (handle, base) = serve(Managers::default()).await;
    let mut sse = Sse::open(&base).await;
    let id = command(&base, new_session("claude")).await["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = sse.until("running", |s| running(s, &id)).await;
    let p = pane(&snap, &id).unwrap();
    let (sid, terminal) = (
        p["sessionId"].as_str().unwrap().to_string(),
        p["terminalId"].as_str().unwrap().to_string(),
    );
    let chat_url = format!(
        "{}/agent/claude/{sid}/ws?token={TOKEN}",
        base.replace("http://", "ws://")
    );
    let (mut chat, _) = tokio_tungstenite::connect_async(chat_url).await.unwrap();

    command(&base, json!({ "type": "closePane", "paneId": id })).await;
    sse.until("the pane gone", |s| pane(s, &id).is_none()).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(msg)) = chat.next().await {
            if let Message::Text(text) = msg {
                let frame: Value = serde_json::from_str(&text).unwrap();
                if frame["t"] == "exit" {
                    return;
                }
            }
        }
        panic!("the chat socket closed without an exit");
    })
    .await
    .expect("an exit frame");
    for _ in 0..50 {
        if !alive_terminals(&base).await.contains(&terminal) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!alive_terminals(&base).await.contains(&terminal));
    handle.stop().await;
}

#[tokio::test]
async fn resuming_a_live_session_returns_its_pane() {
    let _serial = serial().await;
    let (handle, base) = serve(Managers::default()).await;
    let mut sse = Sse::open(&base).await;
    let id = command(&base, new_session("claude")).await["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = sse.until("running", |s| running(s, &id)).await;
    let sid = pane(&snap, &id).unwrap()["sessionId"].clone();
    let terminals = alive_terminals(&base).await.len();

    let mut resume = new_session("claude");
    resume["resume"] = sid;
    let again = command(&base, resume).await;
    assert_eq!(again["paneId"], id.as_str());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        alive_terminals(&base).await.len(),
        terminals,
        "no second process"
    );
    command(&base, json!({ "type": "closePane", "paneId": id })).await;
    handle.stop().await;
}

#[tokio::test]
async fn a_restarted_service_restores_and_respawns_its_panes() {
    let _serial = serial().await;
    let _ = std::fs::remove_file(env().dir.join("workspaces.v2.json"));
    let managers = persistent();
    let (handle, base) = serve(managers.clone()).await;
    let mut sse = Sse::open(&base).await;
    let id = command(&base, new_session("shell")).await["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = sse.until("running", |s| running(s, &id)).await;
    let before = pane(&snap, &id).unwrap()["terminalId"].clone();
    handle.stop().await;
    managers.kill_all();

    let managers = persistent();
    let (handle, base) = serve(managers.clone()).await;
    let mut sse = Sse::open(&base).await;
    let snap = sse.until("respawned", |s| running(s, &id)).await;
    assert_ne!(
        pane(&snap, &id).unwrap()["terminalId"],
        before,
        "a new process"
    );
    command(&base, json!({ "type": "closePane", "paneId": id })).await;
    handle.stop().await;
    managers.kill_all();
    let _ = std::fs::remove_file(env().dir.join("workspaces.v2.json"));
}

#[tokio::test]
async fn every_subscriber_sees_the_same_revs() {
    let _serial = serial().await;
    let (handle, base) = serve(Managers::default()).await;
    let (mut a, mut b) = (Sse::open(&base).await, Sse::open(&base).await);
    let first = command(&base, new_session("shell")).await;
    let tab = first["tabId"].as_str().unwrap().to_string();
    command(
        &base,
        json!({ "type": "rename", "tabId": tab, "label": "one" }),
    )
    .await;
    let last = command(
        &base,
        json!({ "type": "rename", "tabId": tab, "label": "two" }),
    )
    .await;
    let target = last["rev"].as_u64().unwrap();
    let mut revs = Vec::new();
    for sse in [&mut a, &mut b] {
        let mut seen = Vec::new();
        loop {
            let rev = sse.next().await.unwrap()["rev"].as_u64().unwrap();
            seen.push(rev);
            if rev >= target {
                break;
            }
        }
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        revs.push(seen);
    }
    assert_eq!(revs[0], revs[1]);
    command(&base, json!({ "type": "closeTab", "tabId": tab })).await;
    handle.stop().await;
}

#[tokio::test]
async fn a_revoked_listener_ends_the_stream() {
    let _serial = serial().await;
    let (handle, base) = serve(Managers::default()).await;
    let res = reqwest::get(format!("{base}/events/workspace?token=wrong-{TOKEN}"))
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let mut sse = Sse::open(&base).await;
    assert!(sse.next().await.is_some(), "a snapshot on connect");
    handle.stop().await;
    let ended = tokio::time::timeout(Duration::from_secs(10), sse.next()).await;
    assert!(matches!(ended, Ok(None)), "the stream ended");
}

#[tokio::test]
async fn the_server_sets_the_hook_env_not_the_client() {
    let _serial = serial().await;
    let (handle, base) = serve(Managers::default()).await;
    let mut sse = Sse::open(&base).await;
    let mut cmd = new_session("shell");
    cmd["hookSocket"] = "/evil.sock".into();
    cmd["command"] = "printf 'HOOK=%s\\n' \"$WORKBENCH_HOOK_SOCKET\"".into();
    let id = command(&base, cmd).await["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = sse.until("running", |s| running(s, &id)).await;
    let terminal = pane(&snap, &id).unwrap()["terminalId"]
        .as_str()
        .unwrap()
        .to_string();
    let out = terminal_shows(&base, &terminal, "HOOK=127.0.0.1:").await;
    assert!(!out.contains("HOOK=/evil.sock"));

    // Nor may a client claim what a process did.
    let res = reqwest::Client::new()
        .post(format!("{base}/workspace/commands"))
        .bearer_auth(TOKEN)
        .json(&json!({ "type": "sessionAttached", "paneId": id, "sessionId": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    command(&base, json!({ "type": "closePane", "paneId": id })).await;
    handle.stop().await;
}

#[tokio::test]
async fn a_codex_pane_launches_from_the_saved_settings() {
    let _serial = serial().await;
    let (handle, base) = serve(Managers::default()).await;
    let mut sse = Sse::open(&base).await;
    let id = command(&base, new_session("codex")).await["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = sse.until("running", |s| running(s, &id)).await;
    let terminal = pane(&snap, &id).unwrap()["terminalId"]
        .as_str()
        .unwrap()
        .to_string();
    terminal_shows(
        &base,
        &terminal,
        "FAKECODEX -c tui.alternate_screen=never -c approval_policy=never -c sandbox_mode=read-only",
    )
    .await;
    command(&base, json!({ "type": "closePane", "paneId": id })).await;
    handle.stop().await;
}

#[tokio::test]
async fn only_the_desktops_own_listener_opens_native_workspaces() {
    let _serial = serial().await;
    let open = json!({
        "type": "openWorkspace",
        "projectPath": env().project,
        "projectName": "test",
        "renderer": "native",
    });
    let post = |base: String, cmd: Value| async move {
        reqwest::Client::new()
            .post(format!("{base}/workspace/commands"))
            .bearer_auth(TOKEN)
            .json(&cmd)
            .send()
            .await
            .unwrap()
            .status()
    };
    // A server with no native views (standalone, a Linux or Windows desktop).
    let (handle, base) = serve(Managers::default()).await;
    assert_eq!(post(base, open.clone()).await, 400);
    handle.stop().await;

    // The macOS desktop: its loopback listener may, its LAN listener may not.
    let managers = Managers {
        native_views: true,
        ..Managers::default()
    };
    let (loopback, local) = serve(managers.clone()).await;
    let (lan, remote) = serve(managers).await;
    assert_eq!(post(remote, open.clone()).await, 400);
    assert_eq!(post(local, open).await, 200);
    lan.stop().await;
    loopback.stop().await;
}

#[tokio::test]
async fn stopping_a_native_panes_chat_leaves_its_shell() {
    let _serial = serial().await;
    let managers = Managers::default();
    let (handle, base) = serve(managers.clone()).await;
    let sid = "5e5e5e5e-0000-4000-8000-0000000000aa";
    let body = workbench_server::terminal::CreateTerminalBody {
        project_path: env().project.to_string_lossy().into_owned(),
        worktree_path: None,
        name: None,
        command: None,
        claude_session: Some(workbench_server::terminal::ClaudeSessionLaunch {
            id: sid.into(),
            ..Default::default()
        }),
        codex_session: None,
        cols: 80,
        rows: 24,
        pane_id: Some("native-pane".into()),
        shell: None,
        claude_account_id: None,
        native: true,
    };
    let (terminals, agents) = (managers.terminals.clone(), managers.agents.clone());
    let terminal = tokio::task::spawn_blocking(move || {
        workbench_server::terminal::create_from_body(&terminals, &agents, body).unwrap()
    })
    .await
    .unwrap()
    .id;
    for _ in 0..100 {
        if managers.agents.get(sid).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(managers.agents.get(sid).is_some(), "the plugin attached");
    let client = reqwest::Client::new();

    let mode = client
        .post(format!("{base}/agent/claude/{sid}/message"))
        .bearer_auth(TOKEN)
        .body(json!({"t": "mode", "mode": "plan"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(mode.status(), 400);
    assert!(mode.text().await.unwrap().contains("Shift+Tab"));

    let agents = managers.agents.clone();
    let stopped = tokio::task::spawn_blocking(move || agents.stop_pane("native-pane"))
        .await
        .unwrap();
    assert_eq!(stopped, 1);
    assert!(
        alive_terminals(&base).await.contains(&terminal),
        "the person's shell stays"
    );
    managers.terminals.kill(&terminal);
    handle.stop().await;
}

fn persistent() -> Managers {
    Managers {
        workspace: WorkspaceService::persistent(),
        ..Managers::default()
    }
}

#[tokio::test]
async fn an_unreadable_model_is_kept_aside_reported_and_never_saved_over() {
    let _serial = serial().await;
    let file = env().dir.join("workspaces.v2.json");
    let broken = r#"{"version":99,"workspaces":[]}"#;
    std::fs::write(&file, broken).unwrap();
    let managers = persistent();
    let (handle, base) = serve(managers.clone()).await;
    let mut sse = Sse::open(&base).await;
    let snap = sse.until("a snapshot", |_| true).await;
    assert_eq!(snap["persistence"]["status"], "error");
    assert!(snap["persistence"]["message"]
        .as_str()
        .unwrap()
        .contains("workspaces.v2.json"));

    let id = command(&base, new_session("shell")).await["paneId"].clone();
    sse.until("the pane", |s| pane(s, id.as_str().unwrap()).is_some())
        .await;
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        broken,
        "not saved over"
    );
    let copies: Vec<_> = std::fs::read_dir(&env().dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("workspaces.v2.json.broken-")
        })
        .collect();
    assert_eq!(copies.len(), 1);
    handle.stop().await;
    managers.kill_all();
    for copy in copies {
        let _ = std::fs::remove_file(copy.path());
    }
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn a_model_another_process_keeps_is_shown_as_unsaved() {
    let _serial = serial().await;
    let lock = env().dir.join("workspaces.v2.lock");
    // Process 1 is always alive and never us.
    std::fs::write(&lock, "1").unwrap();
    let managers = persistent();
    let (handle, base) = serve(managers.clone()).await;
    let mut sse = Sse::open(&base).await;
    let snap = sse.until("a snapshot", |_| true).await;
    assert_eq!(snap["persistence"]["status"], "locked");
    assert!(snap["persistence"]["message"]
        .as_str()
        .unwrap()
        .contains("pid 1"));
    handle.stop().await;
    managers.kill_all();
    let _ = std::fs::remove_file(&lock);
}

#[tokio::test]
async fn the_migrated_local_state_rides_along_and_every_spawn_bumps_the_generation() {
    let _serial = serial().await;
    let _ = std::fs::remove_file(env().dir.join("workspaces.v2.json"));
    std::fs::write(
        env().dir.join("workspaces.json"),
        json!({
            "workspaces": [{
                "id": "w1", "projectPath": env().project, "projectName": "test",
                "activeTerminalTabId": "t1",
                "terminalTabs": [{ "id": "t1", "label": "Claude 1", "split": "horizontal", "type": "claude",
                    "panes": [{ "id": "p1", "type": "claude", "claudeSessionId": "11111111-1111-1111-1111-111111111111", "view": "chat" }] }]
            }],
            "selectedId": "w1"
        })
        .to_string(),
    )
    .unwrap();
    let managers = persistent();
    let (handle, base) = serve(managers.clone()).await;
    let mut sse = Sse::open(&base).await;
    let snap = sse
        .until("the migrated pane", |s| pane(s, "p1").is_some())
        .await;
    assert_eq!(snap["persistence"]["status"], "ok");
    assert_eq!(snap["local"]["selectedId"], "w1");
    assert_eq!(snap["local"]["chatPanes"], json!(["p1"]));
    let first = pane(&snap, "p1").unwrap()["generation"].as_u64().unwrap();
    assert!(first >= 1, "booted");

    command(&base, json!({ "type": "restart", "tabId": "t1" })).await;
    sse.until("a new generation", |s| {
        pane(s, "p1").and_then(|p| p["generation"].as_u64()) == Some(first + 1)
    })
    .await;
    command(&base, json!({ "type": "closePane", "paneId": "p1" })).await;
    handle.stop().await;
    managers.kill_all();
    let _ = std::fs::remove_file(env().dir.join("workspaces.json"));
    let _ = std::fs::remove_file(env().dir.join("workspaces.v2.json"));
}
