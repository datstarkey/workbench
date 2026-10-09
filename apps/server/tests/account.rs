//! Moving a Claude chat to another Claude account against a fake `claude` (set
//! via `WORKBENCH_CLAUDE_BIN`): the session's JSONL moves to that account's
//! config dir and the terminal restarts there, resuming it. Its own test binary
//! because it points `HOME` at a temp dir.
#![cfg(unix)]

mod support;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{Error, Message};
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";
const SID: &str = "5e5e5e5e-0000-4000-8000-0000000000ac";

/// Logs its login's config dir and argv to `$FAKE_CLAUDE_ARGS`, then idles.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "dir=${CLAUDE_CONFIG_DIR:-default} $*" >> "$FAKE_CLAUDE_ARGS"
while IFS= read -r line; do :; done
"#;

async fn next_json(ws: &mut (impl StreamExt<Item = Result<Message, Error>> + Unpin)) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("frame within 10s")
            .expect("stream open")
            .expect("frame ok");
        if let Message::Text(text) = frame {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

#[tokio::test]
async fn a_chat_moves_to_another_account_and_resumes_there() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-claude.sh");
    std::fs::write(&fake, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        tmp.path().join("projects.json"),
        json!({ "projects": [{ "name": "test", "path": project }] }).to_string(),
    )
    .unwrap();
    let work = tmp.path().join("work-account");
    let broken = tmp.path().join("broken-account");
    std::fs::write(
        tmp.path().join("settings.json"),
        json!({ "claudeAccounts": [
            { "id": "work", "name": "Work", "configDir": work },
            { "id": "broken", "name": "Broken", "configDir": broken },
        ] })
        .to_string(),
    )
    .unwrap();
    // A session the default account has written to, so it is resumed.
    let encoded = workbench_core::paths::encode_project_path(&project.to_string_lossy());
    let history = tmp.path().join(".claude/projects").join(&encoded);
    std::fs::create_dir_all(&history).unwrap();
    std::fs::write(
        history.join(format!("{SID}.jsonl")),
        json!({"type": "user", "uuid": "u1", "sessionId": SID,
               "message": {"role": "user", "content": "hello"}})
        .to_string()
            + "\n",
    )
    .unwrap();
    let args = tmp.path().join("args.log");
    std::env::remove_var("CLAUDE_CONFIG_DIR");
    std::env::set_var("HOME", tmp.path());
    std::env::set_var("WORKBENCH_FAKE_CLAUDE", &fake);
    // Under the broken login `claude` exits before its plugin attaches.
    let wrapper = tmp.path().join("claude-wrapper.sh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ncase \"$CLAUDE_CONFIG_DIR\" in *broken*) exit 1;; esac\nexec '{}' \"$@\"\n",
            support::mod_bridge(tmp.path()).display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &wrapper);
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("FAKE_CLAUDE_ARGS", &args);

    let handle = spawn_embedded("127.0.0.1", 0, Managers::default(), TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let (pane, _) = support::start_claude(&base, &project, SID).await;

    let ws_url = format!("ws://{}/agent/claude/{SID}/ws?token={TOKEN}", handle.addr());
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    assert_eq!(next_json(&mut ws).await["t"], "snapshot");
    ws.send(Message::Text(
        json!({"t": "account", "accountId": "work"}).to_string(),
    ))
    .await
    .unwrap();
    let frame = next_json(&mut ws).await;
    assert_eq!(frame["t"], "replaced", "{frame}");

    for _ in 0..50 {
        if std::fs::read_to_string(&args)
            .unwrap_or_default()
            .lines()
            .count()
            >= 2
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let launches = std::fs::read_to_string(&args).unwrap_or_default();
    let launches: Vec<&str> = launches.lines().collect();
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert!(launches[0].starts_with("dir=default "), "{launches:?}");
    assert!(
        launches[1].starts_with(&format!("dir={} ", work.display())),
        "{launches:?}"
    );
    assert!(
        launches[1].contains(&format!("--resume {SID}")),
        "{launches:?}"
    );
    assert!(work
        .join("projects")
        .join(&encoded)
        .join(format!("{SID}.jsonl"))
        .is_file());
    assert!(!history.join(format!("{SID}.jsonl")).exists());

    let summary = support::wait_for_agent(&base, SID).await;
    assert_eq!(summary["claudeAccountId"], "work", "{summary}");

    // Clients re-attach and learn the login from the snapshot.
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let snapshot = next_json(&mut ws).await;
    assert_eq!(snapshot["t"], "snapshot");
    assert_eq!(snapshot["claudeAccountId"], "work");

    // An unknown account is refused before anything restarts.
    ws.send(Message::Text(
        json!({"t": "account", "accountId": "nope"}).to_string(),
    ))
    .await
    .unwrap();
    let frame = next_json(&mut ws).await;
    assert_eq!(frame["t"], "error", "{frame}");

    // A login whose `claude` never attaches: the files go back and the chat
    // reopens under the account it was on, with its history.
    ws.send(Message::Text(
        json!({"t": "account", "accountId": "broken"}).to_string(),
    ))
    .await
    .unwrap();
    // The socket only hears back once the switch gives up (30s) and reopens.
    for _ in 0..450 {
        if std::fs::read_to_string(&args)
            .unwrap_or_default()
            .lines()
            .count()
            >= 3
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let launches = std::fs::read_to_string(&args).unwrap_or_default();
    let launches: Vec<&str> = launches.lines().collect();
    assert_eq!(launches.len(), 3, "{launches:?}");
    assert!(
        launches[2].starts_with(&format!("dir={} ", work.display())),
        "{launches:?}"
    );
    assert!(
        launches[2].contains(&format!("--resume {SID}")),
        "{launches:?}"
    );
    assert!(work
        .join("projects")
        .join(&encoded)
        .join(format!("{SID}.jsonl"))
        .is_file());
    assert!(!broken
        .join("projects")
        .join(&encoded)
        .join(format!("{SID}.jsonl"))
        .exists());

    support::close_pane(&base, &pane).await;
    handle.stop().await;
}
