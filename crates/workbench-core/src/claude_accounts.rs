//! Several Claude Code logins side by side. Each extra account is its own
//! `CLAUDE_CONFIG_DIR`, which isolates the login (macOS Keychain entry or
//! `.credentials.json`), `.claude.json`, settings, hooks and `projects/`
//! transcripts — verified against Claude Code 2.1.286. The implicit default
//! account (id `None`) is `~/.claude` with no override.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::claude_transcript::{model_options, ModelOption};
use crate::paths;
use crate::types::WorkbenchSettings;

pub const CONFIG_DIR_ENV: &str = "CLAUDE_CONFIG_DIR";

/// Every Claude config dir Workbench manages: the default `~/.claude`, then each
/// account with a usable (absolute) path.
pub fn config_dirs(settings: &WorkbenchSettings) -> Vec<(Option<String>, PathBuf)> {
    let mut dirs = vec![(None, paths::claude_user_dir())];
    dirs.extend(
        settings
            .claude_accounts
            .iter()
            .filter(|a| Path::new(&a.config_dir).is_absolute())
            .map(|a| (Some(a.id.clone()), PathBuf::from(&a.config_dir))),
    );
    dirs
}

/// [`config_dirs`] for the saved settings; just the default when they can't load.
pub fn saved_config_dirs() -> Vec<(Option<String>, PathBuf)> {
    config_dirs(&crate::config::load_workbench_settings().unwrap_or_default())
}

/// The `CLAUDE_CONFIG_DIR` for `account_id`, or `None` for the default account.
/// An id that names no account is an error rather than a silent fallback, so a
/// session never runs (and bills) under the wrong login.
pub fn resolve(settings: &WorkbenchSettings, account_id: Option<&str>) -> Result<Option<PathBuf>> {
    let Some(id) = account_id else {
        return Ok(None);
    };
    let account = settings
        .claude_accounts
        .iter()
        .find(|a| a.id == id)
        .with_context(|| format!("unknown Claude account: {id}"))?;
    let dir = PathBuf::from(&account.config_dir);
    if !dir.is_absolute() {
        bail!("Claude account {id} has a relative config dir; CLAUDE_CONFIG_DIR must be absolute");
    }
    Ok(Some(dir))
}

/// [`resolve`] against the saved settings, creating the dir so a first launch
/// (before `claude auth login`) has somewhere to write.
pub fn resolve_saved(account_id: Option<&str>) -> Result<Option<PathBuf>> {
    if account_id.is_none() {
        return Ok(None);
    }
    let dir = resolve(&crate::config::load_workbench_settings()?, account_id)?;
    if let Some(dir) = &dir {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating Claude config dir {}", dir.display()))?;
    }
    Ok(dir)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAuthStatus {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
}

/// `WORKBENCH_CLAUDE_BIN`, else `claude` found on the enriched search path
/// (GUI apps don't inherit the shell's PATH).
pub fn claude_binary() -> PathBuf {
    let name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    paths::find_binary("WORKBENCH_CLAUDE_BIN", &[name])
}

/// `claude` with `account_id`'s config dir exported.
fn claude_command(account_id: Option<&str>) -> Result<std::process::Command> {
    let mut cmd = crate::shell::tool(claude_binary());
    // A probe is no pane's: its `claude` must not attach to a chat or report hooks.
    crate::shell::without_parent_env(&mut cmd);
    if let Some(dir) = resolve_saved(account_id)? {
        cmd.env(CONFIG_DIR_ENV, dir);
    }
    Ok(cmd)
}

/// Who `account_id` is logged in as, from `claude auth status --json`.
pub fn auth_status(account_id: Option<&str>) -> Result<ClaudeAuthStatus> {
    let output = claude_command(account_id)?
        .args(["auth", "status", "--json"])
        .output()
        .context("Failed to run claude")?;
    // Exits non-zero when logged out but still prints the JSON.
    parse_auth_status(&String::from_utf8_lossy(&output.stdout)).with_context(|| {
        format!(
            "unexpected `claude auth status` output: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })
}

fn parse_auth_status(stdout: &str) -> Result<ClaudeAuthStatus> {
    Ok(serde_json::from_str(stdout.trim())?)
}

/// One plan limit from `/usage`, e.g. label "session", 3%, resets "Oct 1 at 5:10pm (Europe/London)".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimit {
    pub label: String,
    pub percent: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets: Option<String>,
    /// Unix seconds; Codex reports the reset as a time rather than text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
}

/// One rate-limit window a live session's last API response reported, as the
/// Workbench plugin forwards it (`rate_limit_event`'s `windows`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateWindow {
    /// `five_hour`, `seven_day`, or a gateway's `spend_limit`.
    pub kind: String,
    /// 0 to 100, past 100 on an exceeded spend limit.
    pub percent_used: f64,
    /// Unix seconds.
    pub resets_at: Option<u64>,
}

/// Session windows under the labels `/usage` prints, so clients read either
/// source the same way: `five_hour` is "session", `seven_day` "week (all
/// models)", `seven_day_<model>` "week (<Model>)".
pub fn limits_from_windows(windows: &[RateWindow]) -> Vec<UsageLimit> {
    windows
        .iter()
        .map(|w| UsageLimit {
            label: match w.kind.as_str() {
                "five_hour" => "session".to_string(),
                "seven_day" => "week (all models)".to_string(),
                kind => match kind.strip_prefix("seven_day_") {
                    Some(model) => {
                        let mut name = model.replace('_', " ");
                        if let Some(first) = name.get_mut(..1) {
                            first.make_ascii_uppercase();
                        }
                        format!("week ({name})")
                    }
                    None => kind.replace('_', " "),
                },
            },
            percent: w.percent_used.round().clamp(0.0, 255.0) as u8,
            resets: None,
            resets_at: w.resets_at,
        })
        .collect()
}

/// Starting the CLI and asking Anthropic for the numbers takes a couple of seconds.
const USAGE_TIMEOUT: Duration = Duration::from_secs(20);

/// `account_id`'s plan limits, from `claude -p /usage` — the same server-side
/// figures as `/usage` in a session, so they include other devices and
/// claude.ai. Empty when logged out, on an API key (no plan limits), or if the
/// CLI's wording changes. `--no-session-persistence` keeps each check from
/// writing a transcript; the temp-dir cwd keeps it out of any project.
pub fn usage(account_id: Option<&str>) -> Result<Vec<UsageLimit>> {
    let mut cmd = claude_command(account_id)?;
    cmd.args(["-p", "/usage", "--no-session-persistence"])
        .current_dir(std::env::temp_dir());
    let stdout = crate::shell::output_with_timeout(&mut cmd, USAGE_TIMEOUT)
        .context("`claude -p /usage` failed or timed out")?;
    Ok(parse_usage(&stdout))
}

/// The CLI answers `initialize` in about a second; slow MCP start-ups add to it.
const MODELS_TIMEOUT: Duration = Duration::from_secs(20);

/// The models `account_id` can pick in `cwd` (project settings can narrow
/// them), each with its effort levels, from the CLI's `initialize` reply. Only
/// the handshake is sent, so no model is called and nothing is billed; hooks
/// are off so a probe doesn't run the person's `SessionStart` hooks. Closing
/// stdin then ends the CLI, which stops its MCP servers itself; it is killed
/// only if it hangs.
pub fn models(account_id: Option<&str>, cwd: &Path) -> Result<Vec<ModelOption>> {
    let mut cmd = claude_command(account_id)?;
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ])
    .current_dir(cwd)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    let mut child = cmd.spawn().context("Failed to run claude")?;
    let reply = initialize_reply(&mut child);
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(5);
    while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let reply = reply?;
    let models = reply
        .pointer("/response/response/models")
        .and_then(Value::as_array)
        .with_context(|| format!("`claude` listed no models: {}", reply["response"]))?;
    Ok(model_options(models))
}

/// Send `initialize` and wait for its reply; the caller reaps the child either way.
fn initialize_reply(child: &mut std::process::Child) -> Result<Value> {
    let hello = json!({"type": "control_request", "request_id": "models",
        "request": {"subtype": "initialize"}});
    writeln!(child.stdin.as_mut().context("no stdin")?, "{hello}")?;
    let stdout = child.stdout.take().context("no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let reply = BufReader::new(stdout)
            .lines()
            .map_while(std::io::Result::ok)
            .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
            .find(|v| v.get("type").and_then(Value::as_str) == Some("control_response"));
        let _ = tx.send(reply);
    });
    rx.recv_timeout(MODELS_TIMEOUT)
        .ok()
        .flatten()
        .context("`claude` didn't answer `initialize`")
}

/// Picks `Current <label>: <n>% used[ · resets <when>]` lines out of the text.
fn parse_usage(stdout: &str) -> Vec<UsageLimit> {
    stdout
        .lines()
        .filter_map(|line| {
            let (label, rest) = line.trim().strip_prefix("Current ")?.split_once(": ")?;
            let (percent, rest) = rest.split_once("% used")?;
            let resets = rest
                .split_once("resets ")
                .map(|(_, when)| when.trim().to_string());
            Some(UsageLimit {
                label: label.to_string(),
                percent: percent.trim().parse().ok()?,
                resets,
                resets_at: None,
            })
        })
        .collect()
}

/// Move a session to another account's config dir (`None` is `~/.claude`) so
/// that login `--resume`s it: its JSONL, the folder beside it (subagents, tool
/// results), its file checkpoints, session env and todos. A move, not a copy:
/// one id under two accounts is found under either. A session with no JSONL
/// yet has nothing to move.
pub fn move_session(from: Option<&Path>, to: Option<&Path>, session_id: &str) -> Result<()> {
    let root = |dir: Option<&Path>| dir.map(Path::to_path_buf).unwrap_or_else(paths::claude_user_dir);
    let (from, to) = (root(from), root(to));
    if from == to {
        return Ok(());
    }
    let Some(jsonl) = crate::claude_transcript::find_transcript(&from.join("projects"), session_id)
    else {
        return Ok(());
    };
    if crate::claude_transcript::find_transcript(&to.join("projects"), session_id).is_some() {
        bail!("the other account already has a session {session_id}");
    }
    let project = jsonl
        .parent()
        .and_then(Path::file_name)
        .context("session transcript outside a project folder")?;
    let dest = to.join("projects").join(project);
    std::fs::create_dir_all(&dest)
        .with_context(|| format!("creating {}", dest.display()))?;
    let moved_jsonl = dest.join(format!("{session_id}.jsonl"));
    // Config dirs on different volumes can't rename across: copy, then remove.
    std::fs::rename(&jsonl, &moved_jsonl)
        .or_else(|_| {
            std::fs::copy(&jsonl, &moved_jsonl)?;
            std::fs::remove_file(&jsonl)
        })
        .with_context(|| format!("moving {}", jsonl.display()))?;
    // The transcript is what `--resume` needs; the rest only enriches it.
    let mut extras = vec![(jsonl.with_extension(""), dest.join(session_id))];
    for dir in ["file-history", "session-env"] {
        extras.push((from.join(dir).join(session_id), to.join(dir).join(session_id)));
    }
    // `todos/<session>-agent-<agent>.json`, one per agent of the session.
    if let Ok(entries) = std::fs::read_dir(from.join("todos")) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(session_id) {
                extras.push((entry.path(), to.join("todos").join(entry.file_name())));
            }
        }
    }
    for (src, dst) in extras {
        if !src.exists() || dst.exists() {
            continue;
        }
        let moved = dst
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::rename(&src, &dst));
        if let Err(e) = moved {
            log::warn!("moving {} to another account: {e}", src.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ClaudeAccount;

    #[test]
    fn move_session_takes_the_transcript_and_its_folders() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "0b5a3c1e-9d2f-4e7a-8c6b-1f2e3d4c5b6a";
        let project = a.path().join("projects/-repo");
        std::fs::create_dir_all(project.join(id).join("subagents")).unwrap();
        std::fs::write(project.join(format!("{id}.jsonl")), "{}\n").unwrap();
        std::fs::create_dir_all(a.path().join("file-history").join(id)).unwrap();
        std::fs::create_dir_all(a.path().join("session-env").join(id)).unwrap();
        std::fs::create_dir_all(a.path().join("todos")).unwrap();
        let todo = format!("{id}-agent-{id}.json");
        std::fs::write(a.path().join("todos").join(&todo), "[]").unwrap();
        std::fs::write(a.path().join("todos/other-agent-x.json"), "[]").unwrap();

        move_session(Some(a.path()), Some(b.path()), id).unwrap();

        let moved = b.path().join("projects/-repo");
        assert!(moved.join(format!("{id}.jsonl")).is_file());
        assert!(moved.join(id).join("subagents").is_dir());
        assert!(b.path().join("file-history").join(id).is_dir());
        assert!(b.path().join("session-env").join(id).is_dir());
        assert!(b.path().join("todos").join(&todo).is_file());
        assert!(a.path().join("todos/other-agent-x.json").is_file());
        assert!(!project.join(format!("{id}.jsonl")).exists());
        assert!(!project.join(id).exists());
    }

    #[test]
    fn move_session_refuses_an_id_the_other_account_has() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = "0b5a3c1e-9d2f-4e7a-8c6b-1f2e3d4c5b6a";
        for dir in [a.path(), b.path()] {
            std::fs::create_dir_all(dir.join("projects/-repo")).unwrap();
            std::fs::write(dir.join(format!("projects/-repo/{id}.jsonl")), "{}\n").unwrap();
        }
        assert!(move_session(Some(a.path()), Some(b.path()), id).is_err());
        assert!(a.path().join(format!("projects/-repo/{id}.jsonl")).is_file());
    }

    #[test]
    fn move_session_without_a_transcript_is_a_no_op() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        move_session(Some(a.path()), Some(b.path()), "0b5a3c1e-9d2f-4e7a-8c6b-1f2e3d4c5b6a")
            .unwrap();
    }

    #[test]
    fn probes_never_inherit_a_panes_wiring() {
        let cmd = claude_command(None).unwrap();
        let removed: Vec<_> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for key in crate::shell::PARENT_ONLY_ENV {
            assert!(removed.iter().any(|k| k == key), "{key} is passed on");
        }
    }

    fn settings_with(config_dir: &str) -> WorkbenchSettings {
        WorkbenchSettings {
            claude_accounts: vec![ClaudeAccount {
                id: "work".into(),
                name: "Work".into(),
                config_dir: config_dir.into(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn default_account_needs_no_override() {
        assert_eq!(resolve(&WorkbenchSettings::default(), None).unwrap(), None);
    }

    #[test]
    fn resolves_a_known_account_to_its_dir() {
        let dir = std::env::temp_dir().join("claude-work");
        let settings = settings_with(dir.to_str().unwrap());
        assert_eq!(resolve(&settings, Some("work")).unwrap(), Some(dir));
    }

    #[test]
    fn rejects_unknown_ids_and_relative_dirs() {
        let settings = settings_with("relative/dir");
        assert!(resolve(&settings, Some("nope")).is_err());
        assert!(resolve(&settings, Some("work")).is_err());
    }

    #[test]
    fn config_dirs_lists_default_first_and_skips_relative_dirs() {
        let dirs = config_dirs(&settings_with("relative/dir"));
        assert_eq!(dirs, vec![(None, paths::claude_user_dir())]);

        let abs = std::env::temp_dir().join("claude-work");
        let dirs = config_dirs(&settings_with(abs.to_str().unwrap()));
        assert_eq!(dirs[1], (Some("work".to_string()), abs));
    }

    #[test]
    fn parses_logged_in_and_logged_out_status() {
        let status = parse_auth_status(
            r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@b.c","orgName":"Org","subscriptionType":"max","configDirectory":"/x"}"#,
        )
        .unwrap();
        assert!(status.logged_in);
        assert_eq!(status.email.as_deref(), Some("a@b.c"));
        assert_eq!(status.subscription_type.as_deref(), Some("max"));

        let out = parse_auth_status("{\n  \"loggedIn\": false,\n  \"authMethod\": \"none\"\n}\n")
            .unwrap();
        assert_eq!(out, ClaudeAuthStatus::default());
    }

    /// Verbatim `claude -p /usage` output from Claude Code 2.1.286.
    #[test]
    fn parses_plan_limits_from_usage_output() {
        let out = "You are currently using your subscription to power your Claude Code usage\n\n\
Current session: 3% used · resets Oct 1 at 5:10pm (Europe/London)\n\
Current week (all models): 89% used · resets Oct 2 at 9am (Europe/London)\n\
Current week (Fable): 0% used · resets Oct 2 at 9am (Europe/London)\n\n\
What's contributing to your limits usage?\n\
Last 24h · 3270 requests · 24 sessions\n  91% of your usage came from subagent-heavy sessions\n";
        let limits = parse_usage(out);
        assert_eq!(limits.len(), 3);
        assert_eq!(
            limits[0],
            UsageLimit {
                label: "session".into(),
                percent: 3,
                resets: Some("Oct 1 at 5:10pm (Europe/London)".into()),
                resets_at: None,
            }
        );
        assert_eq!(limits[1].label, "week (all models)");
        assert_eq!(limits[1].percent, 89);
    }

    #[test]
    fn session_windows_take_the_usage_labels() {
        let windows: Vec<RateWindow> = serde_json::from_value(json!([
            {"kind": "five_hour", "percentUsed": 23.5, "resetsAt": 1_800_000_000u64},
            {"kind": "seven_day", "percentUsed": 89},
            {"kind": "seven_day_opus", "percentUsed": 4},
            {"kind": "spend_limit", "percentUsed": 312.4}
        ]))
        .unwrap();
        let limits = limits_from_windows(&windows);
        assert_eq!(
            limits[0],
            UsageLimit {
                label: "session".into(),
                percent: 24,
                resets: None,
                resets_at: Some(1_800_000_000),
            }
        );
        let labels: Vec<_> = limits.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(
            labels,
            ["session", "week (all models)", "week (Opus)", "spend limit"]
        );
        assert_eq!(limits[3].percent, 255, "an exceeded spend limit is clamped");
    }

    /// Logged out (or on an API key) `/usage` prints only a cost summary.
    #[test]
    fn usage_without_plan_limits_is_empty() {
        let out = "Total cost:            $0.0000\nUsage:                 0 input, 0 output\n";
        assert!(parse_usage(out).is_empty());
    }
}
