//! What differs per CLI, behind one interface: how the lines a session gets
//! (a terminal plugin's posts, `codex app-server`'s stdout) fold into chat
//! items, and how client messages are encoded for it. Drivers are state
//! machines — they return messages to send and say what changed;
//! [`super::AgentSession`] does the IO.

use std::process::Command;

use anyhow::Result;
use serde_json::{Map, Value};
use workbench_core::claude_transcript::{
    ApprovalDecision, ChatView, ElicitationAction, Transcript,
};

use super::codex::CodexDriver;
use super::{claude, PromptFile, PromptImage};

// One per session, behind its mutex: the variants' sizes don't matter.
#[allow(clippy::large_enum_variant)]
pub(super) enum Driver {
    Claude(Transcript),
    Codex(CodexDriver),
}

/// What applying a line or a client message did.
#[derive(Debug, Default)]
pub(super) struct Effects {
    /// Lines for the CLI's stdin, in order.
    pub send: Vec<Value>,
    pub frames: Vec<Value>,
    /// Item indices added or changed.
    pub items: Vec<usize>,
    /// Broadcast an update even if no item changed.
    pub meta: bool,
    /// The slash command list changed.
    pub commands: bool,
    /// The conversation continues under a new id (Claude's `/clear`).
    pub new_id: Option<String>,
    /// That id is another conversation (`/resume`), not a continuation.
    pub resumed: bool,
    /// Items were replaced wholesale (history loaded): send a fresh snapshot.
    pub snapshot: bool,
    /// The session is ready under this id, or failed to start.
    pub ready: Option<Result<String, String>>,
}

/// A process to spawn, the driver for it, and its opening lines.
pub(super) struct Launch {
    pub cmd: Command,
    pub driver: Driver,
    pub hello: Vec<Value>,
    /// The CLI, for messages.
    pub program: &'static str,
}

impl Driver {
    pub fn codex_action(
        &mut self,
        request_id: &str,
        action: workbench_core::codex_controls::Action,
        params: &Value,
    ) -> Result<Effects> {
        match self {
            Self::Codex(c) => c.action(request_id, action, params),
            Self::Claude(_) => anyhow::bail!("this action needs a Codex session"),
        }
    }

    pub fn tick(&mut self) -> Effects {
        match self {
            Self::Codex(c) => c.tick(),
            Self::Claude(_) => Effects::default(),
        }
    }
    pub fn artifacts(&self, id: &str) -> Option<&[Value]> {
        match self {
            Self::Codex(c) => c.transcript().artifacts(id),
            _ => None,
        }
    }
    pub fn view(&self) -> &dyn ChatView {
        match self {
            Self::Claude(t) => t,
            Self::Codex(c) => c.transcript(),
        }
    }

    pub fn apply_line(&mut self, line: &str) -> Effects {
        match self {
            Self::Claude(t) => claude::apply_line(t, line),
            Self::Codex(c) => c.apply_line(line),
        }
    }

    pub fn prompt(
        &mut self,
        text: &str,
        images: &[PromptImage],
        files: &[PromptFile],
        now: bool,
    ) -> Result<Effects> {
        match self {
            Self::Claude(_) if !(images.is_empty() && files.is_empty()) => {
                anyhow::bail!("a Claude chat takes attachments as `@path` mentions")
            }
            Self::Claude(t) => Ok(claude::prompt(t, text, &[], now)),
            Self::Codex(c) => c.prompt(text, images, files),
        }
    }

    /// A Claude terminal session's prompt naming the files saved for it
    /// (`workbench_attachments`), which its plugin lists for Claude to Read.
    pub fn prompt_attached(&mut self, text: &str, files: &[String], now: bool) -> Result<Effects> {
        match self {
            Self::Claude(t) => Ok(claude::prompt(t, text, files, now)),
            Self::Codex(_) => anyhow::bail!("a Codex chat takes attachments as uploads"),
        }
    }

    pub fn approve(
        &mut self,
        request_id: &str,
        decision: ApprovalDecision,
        answers: Option<&Map<String, Value>>,
    ) -> Result<Effects> {
        match self {
            Self::Claude(t) => Ok(claude::approve(t, request_id, decision, answers)),
            Self::Codex(c) => c.approve(request_id, decision, answers),
        }
    }

    pub fn elicit(
        &mut self,
        request_id: &str,
        action: ElicitationAction,
        content: Option<&Map<String, Value>>,
    ) -> Result<Effects> {
        let resolved = match self {
            Self::Claude(_) => anyhow::bail!("Answer it in the terminal."),
            Self::Codex(c) => c.resolve_elicitation(request_id, action, content)?,
        };
        // `None`: already answered (another device, or twice).
        Ok(resolved
            .map(|(i, response)| Effects {
                send: vec![response],
                items: vec![i],
                ..Effects::default()
            })
            .unwrap_or_default())
    }

    pub fn interrupt(&mut self) -> Result<Effects> {
        match self {
            Self::Claude(_) => Ok(claude::interrupt()),
            Self::Codex(c) => Ok(c.interrupt()),
        }
    }

    /// Codex switches in place; a Claude terminal restarts in the mode
    /// (`AgentManager::mode_terminal`).
    pub fn set_mode(&mut self, mode: &str) -> Result<Effects> {
        match self {
            Self::Claude(_) => anyhow::bail!("Switch it in the terminal with Shift+Tab."),
            Self::Codex(c) => c.set_mode(mode),
        }
    }

    pub fn set_model(&mut self, model: &str) -> Result<Effects> {
        match self {
            Self::Claude(t) => claude::set_model(t, model),
            Self::Codex(c) => c.set_model(model),
        }
    }

    pub fn set_effort(&mut self, level: &str) -> Result<Effects> {
        match self {
            Self::Claude(t) => claude::set_effort(t, level),
            Self::Codex(c) => c.set_effort(level),
        }
    }
}
