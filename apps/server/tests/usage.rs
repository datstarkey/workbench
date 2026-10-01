//! `GET /agent/usage` against a fake `claude` that prints verbatim `/usage`
//! output and counts its runs. One test, since it sets process-global env.
#![cfg(unix)]

use serde_json::{json, Value};
use std::time::Duration;

use workbench_server::usage::UsageCache;
use workbench_server::{spawn_embedded, Managers};

const TOKEN: &str = "e2e-token-0123456789abcdef0123456789";

/// Claude Code 2.1.286's `claude -p /usage` text; a non-default account
/// (`CLAUDE_CONFIG_DIR` set) reports a different session figure.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo run >> "$WORKBENCH_TEST_RUNS"
echo "You are currently using your subscription to power your Claude Code usage"
echo
if [ -n "$CLAUDE_CONFIG_DIR" ]; then pct=7; else pct=3; fi
echo "Current session: $pct% used · resets Oct 1 at 5:10pm (Europe/London)"
echo "Current week (all models): 89% used · resets Oct 2 at 9am (Europe/London)"
echo "Current week (Fable): 0% used · resets Oct 2 at 9am (Europe/London)"
"#;

#[tokio::test]
async fn usage_is_parsed_cached_per_account_and_refuses_unknown_accounts() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("fake-claude.sh");
    std::fs::write(&fake, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runs = tmp.path().join("runs");
    let work_dir = tmp.path().join("claude-work");
    std::fs::write(
        tmp.path().join("settings.json"),
        json!({ "claudeAccounts": [{ "id": "work", "name": "Work", "configDir": work_dir }] })
            .to_string(),
    )
    .unwrap();
    std::env::set_var("WORKBENCH_CLAUDE_BIN", &fake);
    std::env::set_var("WORKBENCH_CONFIG_DIR", tmp.path());
    std::env::set_var("WORKBENCH_TEST_RUNS", &runs);

    let managers = Managers {
        usage: UsageCache::with_fresh_ttl(Duration::from_millis(300)),
        ..Managers::default()
    };
    let handle = spawn_embedded("127.0.0.1", 0, managers, TOKEN.to_string())
        .await
        .expect("server should bind");
    let base = format!("http://{}", handle.addr());
    let http = reqwest::Client::new();
    let get = |query: &'static str| {
        http.get(format!("{base}/agent/usage{query}"))
            .bearer_auth(TOKEN)
            .send()
    };
    let run_count = || {
        std::fs::read_to_string(&runs)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    };

    let (a, b) = tokio::join!(get(""), get(""));
    let first: Value = a.unwrap().json().await.unwrap();
    assert_eq!(b.unwrap().json::<Value>().await.unwrap(), first);
    assert_eq!(
        first,
        json!([
            { "label": "session", "percent": 3, "resets": "Oct 1 at 5:10pm (Europe/London)" },
            { "label": "week (all models)", "percent": 89, "resets": "Oct 2 at 9am (Europe/London)" },
            { "label": "week (Fable)", "percent": 0, "resets": "Oct 2 at 9am (Europe/London)" }
        ])
    );
    assert_eq!(run_count(), 1, "concurrent requests share one CLI run");

    let cached: Value = get("").await.unwrap().json().await.unwrap();
    assert_eq!(cached, first);
    assert_eq!(run_count(), 1, "a second request within the TTL is cached");

    // A fresh request (a turn ended) shares a run from the last moments, then reruns.
    get("?fresh=true").await.unwrap();
    assert_eq!(
        run_count(),
        1,
        "a fresh request within the floor shares the run"
    );
    tokio::time::sleep(Duration::from_millis(350)).await;
    let (a, b) = tokio::join!(get("?fresh=true"), get("?fresh=true"));
    assert_eq!(a.unwrap().status(), 200);
    assert_eq!(b.unwrap().status(), 200);
    assert_eq!(
        run_count(),
        2,
        "past the floor, a burst of fresh requests runs once"
    );
    get("").await.unwrap();
    assert_eq!(run_count(), 2, "a normal request takes the fresh result");

    let work: Value = get("?claudeAccountId=work")
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(work[0]["percent"], 7, "the account's own login is checked");
    assert_eq!(run_count(), 3);

    let unknown = get("?claudeAccountId=nope").await.unwrap();
    assert_eq!(unknown.status(), 400);
    assert_eq!(run_count(), 3, "an unknown account never runs the CLI");

    let anon = reqwest::Client::new()
        .get(format!("{base}/agent/usage"))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);

    handle.stop().await;
}
