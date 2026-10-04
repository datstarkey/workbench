//! Versioned, allowlisted Codex chat controls (app-server 0.160).
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptions {
    pub codex_approval_policy: Option<String>,
    pub codex_sandbox_mode: Option<String>,
}
impl LaunchOptions {
    pub fn validate(&self) -> Result<()> {
        if self
            .codex_approval_policy
            .as_deref()
            .is_some_and(|v| !["never", "on-request", "untrusted", "on-failure"].contains(&v))
        {
            bail!("unknown Codex approval policy");
        }
        if self
            .codex_sandbox_mode
            .as_deref()
            .is_some_and(|v| !["read-only", "workspace-write", "danger-full-access"].contains(&v))
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
    AttachmentAdd,
    AttachmentRemove,
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
}
