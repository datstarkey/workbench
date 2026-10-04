//! What differs per CLI, behind one interface: how a session's process is
//! started, how its stdout folds into chat items, and how client messages are
//! encoded for its stdin. Drivers are state machines — they return messages to
//! write and say what changed; [`super::AgentSession`] does the IO.

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
    /// Item indices added or changed.
    pub items: Vec<usize>,
    /// Broadcast an update even if no item changed.
    pub meta: bool,
    /// The slash command list changed.
    pub commands: bool,
    /// The conversation continues under a new id (Claude's `/clear`).
    pub new_id: Option<String>,
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
    /// Set when the id is known and nothing more is needed before clients use it.
    pub ready: Option<String>,
    /// For error messages: `claude` / `codex`.
    pub program: &'static str,
}

impl Driver {
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
    ) -> Result<Effects> {
        match self {
            Self::Claude(t) => claude::prompt(t, text, images, files),
            Self::Codex(c) => c.prompt(text, images, files),
        }
    }

    pub fn approve(
        &mut self,
        request_id: &str,
        decision: ApprovalDecision,
        answers: Option<&Map<String, Value>>,
    ) -> Result<Effects> {
        match self {
            Self::Claude(t) => claude::approve(t, request_id, decision, answers),
            Self::Codex(c) => Ok(c.approve(request_id, decision, answers)),
        }
    }

    pub fn elicit(
        &mut self,
        request_id: &str,
        action: ElicitationAction,
        content: Option<&Map<String, Value>>,
    ) -> Effects {
        let resolved = match self {
            Self::Claude(t) => t.resolve_elicitation(request_id, action, content),
            Self::Codex(c) => c.resolve_elicitation(request_id, action, content),
        };
        // `None`: already answered (another device, or twice).
        resolved
            .map(|(i, response)| Effects {
                send: vec![response],
                items: vec![i],
                ..Effects::default()
            })
            .unwrap_or_default()
    }

    pub fn interrupt(&mut self) -> Result<Effects> {
        match self {
            Self::Claude(_) => Ok(claude::interrupt()),
            Self::Codex(c) => Ok(c.interrupt()),
        }
    }

    pub fn set_mode(&mut self, mode: &str) -> Result<Effects> {
        match self {
            Self::Claude(t) => claude::set_mode(t, mode),
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
