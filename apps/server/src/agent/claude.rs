//! The Claude driver: the stream-json a Claude session speaks, folded by
//! [`Transcript`]. The process is an interactive `claude` in a server
//! terminal; the Workbench plugin translates it to and from stream-json
//! (`modlink`), so the chat and the terminal are one process. The session id
//! is the Claude session id.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use workbench_core::claude_transcript::{self, ApprovalDecision, Transcript};

use super::driver::Effects;

/// Effort levels `effortLevel` accepts.
const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

pub(crate) fn validate(session_id: &str) -> Result<()> {
    if !claude_transcript::is_uuid(session_id) {
        bail!("session id must be a UUID");
    }
    Ok(())
}

/// The session's JSONL, once the CLI has written one.
pub(super) fn history(config_dir: Option<&Path>, session_id: &str) -> Option<PathBuf> {
    let projects = config_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(workbench_core::paths::claude_user_dir)
        .join("projects");
    claude_transcript::find_transcript(&projects, session_id)
}

/// A transcript holding the session's history, for a terminal's `claude`
/// the plugin feeds (no process is started).
pub(super) fn history_transcript(
    config_dir: Option<&Path>,
    session_id: &str,
    resume_at: Option<&str>,
) -> Transcript {
    history(config_dir, session_id)
        .as_deref()
        .map(|path| Transcript::load_at(path, resume_at))
        .unwrap_or_default()
}

/// The SDK handshake; its reply lists the models the chat's picker offers.
pub(super) fn hello() -> Value {
    control(json!({"subtype": "initialize"}))
}

fn control(request: Value) -> Value {
    json!({
        "type": "control_request",
        "request_id": uuid::Uuid::new_v4().to_string(),
        "request": request,
    })
}

pub(super) fn apply_line(t: &mut Transcript, line: &str) -> Effects {
    let applied = t.apply_line(line);
    if let Some(kind) = &applied.unknown_kind {
        tracing::warn!("claude sent an unrecognised message kind: {kind}");
    }
    Effects {
        items: applied.items,
        meta: applied.meta,
        commands: applied.commands,
        new_id: applied.new_session_id,
        resumed: applied.resumed,
        ..Effects::default()
    }
}

/// Attachments are already `@path` mentions in `text` (see `attachment`).
pub(super) fn prompt(t: &mut Transcript, text: &str) -> Effects {
    t.set_busy();
    Effects {
        send: vec![json!({"type": "user", "message": {"role": "user", "content": text}})],
        meta: true,
        ..Effects::default()
    }
}

pub(super) fn approve(
    t: &mut Transcript,
    request_id: &str,
    decision: ApprovalDecision,
    answers: Option<&Map<String, Value>>,
) -> Effects {
    // `None`: already answered (another device, or twice).
    match t.resolve_approval(request_id, decision, answers) {
        Some((i, response)) => Effects {
            send: vec![response],
            items: vec![i],
            ..Effects::default()
        },
        None => Effects::default(),
    }
}

pub(super) fn interrupt() -> Effects {
    Effects {
        send: vec![control(json!({"subtype": "interrupt"}))],
        ..Effects::default()
    }
}

/// Shown at once; the transcript puts the old model back if the CLI refuses.
pub(super) fn set_model(t: &mut Transcript, model: &str) -> Result<Effects> {
    let valid = !model.is_empty()
        && model.len() <= 80
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._[]".contains(&b));
    if !valid {
        bail!("unknown model: {model}");
    }
    let msg = control(json!({"subtype": "set_model", "model": model}));
    t.request_model(msg["request_id"].as_str().unwrap_or_default(), model);
    Ok(changed_meta(msg))
}

pub(super) fn set_effort(t: &mut Transcript, level: &str) -> Result<Effects> {
    if !EFFORT_LEVELS.contains(&level) {
        bail!("unknown effort level: {level}");
    }
    t.set_effort(level);
    Ok(changed_meta(control(
        json!({"subtype": "apply_flag_settings", "settings": {"effortLevel": level}}),
    )))
}

fn changed_meta(msg: Value) -> Effects {
    Effects {
        send: vec![msg],
        meta: true,
        ..Effects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workbench_core::claude_transcript::ChatView;

    #[test]
    fn a_refused_model_switch_is_rolled_back() {
        let mut t = Transcript::default();
        t.apply(&json!({"type": "system", "subtype": "init", "model": "claude-opus-5-5"}));
        let fx = set_model(&mut t, "sonnet").unwrap();
        assert_eq!(t.meta().model_choice.as_deref(), Some("sonnet"));
        let id = fx.send[0]["request_id"].as_str().unwrap();
        t.apply(&json!({"type": "control_response", "response": {
            "subtype": "error", "request_id": id, "error": "Model: no"}}));
        assert_eq!(t.meta().model_choice, None);
        assert!(set_model(&mut t, "rm -rf").is_err());
    }
}
