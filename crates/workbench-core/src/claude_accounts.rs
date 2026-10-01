//! Several Claude Code logins side by side. Each extra account is its own
//! `CLAUDE_CONFIG_DIR`, which isolates the login (macOS Keychain entry or
//! `.credentials.json`), `.claude.json`, settings, hooks and `projects/`
//! transcripts — verified against Claude Code 2.1.286. The implicit default
//! account (id `None`) is `~/.claude` with no override.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

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
    let mut cmd = crate::shell::command(claude_binary());
    cmd.env("PATH", paths::enriched_path());
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

/// Starting the CLI and asking Anthropic for the numbers takes a couple of seconds.
const USAGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ClaudeAccount;

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

    /// Logged out (or on an API key) `/usage` prints only a cost summary.
    #[test]
    fn usage_without_plan_limits_is_empty() {
        let out = "Total cost:            $0.0000\nUsage:                 0 input, 0 output\n";
        assert!(parse_usage(out).is_empty());
    }
}
