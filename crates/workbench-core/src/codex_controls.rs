//! Versioned, allowlisted Codex chat controls (app-server 0.160).
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Values Codex takes for `approval_policy` and `sandbox_mode` overrides.
pub const APPROVAL_POLICIES: &[&str] = &["never", "on-request", "untrusted", "on-failure"];
pub const SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptions {
    pub codex_approval_policy: Option<String>,
    pub codex_sandbox_mode: Option<String>,
}
impl LaunchOptions {
    /// Resolve missing overrides on the server so every client shares saved defaults.
    pub fn with_defaults(mut self, approval: &str, sandbox: &str) -> Self {
        if self.codex_approval_policy.is_none() && approval != "default" {
            self.codex_approval_policy = Some(approval.into());
        }
        if self.codex_sandbox_mode.is_none() && sandbox != "default" {
            self.codex_sandbox_mode = Some(sandbox.into());
        }
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self
            .codex_approval_policy
            .as_deref()
            .is_some_and(|v| !APPROVAL_POLICIES.contains(&v))
        {
            bail!("unknown Codex approval policy");
        }
        if self
            .codex_sandbox_mode
            .as_deref()
            .is_some_and(|v| !SANDBOX_MODES.contains(&v))
        {
            bail!("unknown Codex sandbox mode");
        }
        Ok(())
    }
}

/// No raw RPC proxy: the server builds each request using its own cwd/thread.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Compact,
    Review,
    Fork,
    Rename,
    Archive,
    Unarchive,
    Threads,
    History,
    Collaboration,
    ServiceTier,
    Goal,
    ClearGoal,
    QueueAdd,
    QueueUpdate,
    QueueDelete,
    QueueReorder,
    QueueSend,
    QueuePause,
    Inspect,
    Login,
    CancelLogin,
    McpLogin,
    RemoteEnable,
    RemoteDisable,
    RemotePair,
    RemoteRevoke,
    BackgroundTerminate,
    BackgroundClean,
    RealtimeStart,
    RealtimeStop,
    RealtimeAudio,
    RealtimeText,
    Elicitation,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub approval_policy: Option<Value>,
    pub sandbox: Option<Value>,
    pub capabilities: Vec<String>,
    pub collaboration_modes: Vec<Value>,
    pub collaboration_mode: Option<String>,
    pub service_tier: Option<String>,
    pub goal: Option<Value>,
    pub queue: Vec<QueuedPrompt>,
    pub queue_paused: bool,
    pub has_older_history: bool,
    pub realtime: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedPrompt {
    pub id: String,
    pub text: String,
    pub images: usize,
    pub files: Vec<String>,
    /// Uploaded file mentions kept when the queued message's text is edited.
    #[serde(skip)]
    pub file_context: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launch_defaults_are_independent_and_explicit_choices_win() {
        let options = LaunchOptions::default().with_defaults("never", "read-only");
        assert_eq!(options.codex_approval_policy.as_deref(), Some("never"));
        assert_eq!(options.codex_sandbox_mode.as_deref(), Some("read-only"));
        let options = LaunchOptions {
            codex_approval_policy: Some("untrusted".into()),
            codex_sandbox_mode: None,
        }
        .with_defaults("never", "workspace-write");
        assert_eq!(options.codex_approval_policy.as_deref(), Some("untrusted"));
        assert_eq!(
            options.codex_sandbox_mode.as_deref(),
            Some("workspace-write")
        );
        let options = LaunchOptions::default().with_defaults("default", "default");
        assert!(options.codex_approval_policy.is_none());
        assert!(options.codex_sandbox_mode.is_none());
        let options = LaunchOptions::default().with_defaults("on-request", "default");
        assert_eq!(options.codex_approval_policy.as_deref(), Some("on-request"));
        assert!(options.codex_sandbox_mode.is_none());
    }
}
