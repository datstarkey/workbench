//! The Claude driver: `claude -p` speaking stream-json, events folded by
//! [`Transcript`]. The session id is the Claude session id, so the same
//! conversation can move between chat and a terminal running
//! `claude --resume <id>` — one process at a time.

use std::path::Path;

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use workbench_core::claude_accounts;
use workbench_core::claude_launch::PERMISSION_MODES;
use workbench_core::claude_transcript::{self, ApprovalDecision, Transcript};

use super::driver::{Driver, Effects, Launch};
use super::{PromptImage, StartAgent};

/// Effort levels `effortLevel` accepts.
const EFFORT_LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

pub(super) fn validate(session_id: &str, permission_mode: Option<&str>) -> Result<()> {
    if !claude_transcript::is_uuid(session_id) {
        bail!("session id must be a UUID");
    }
    if let Some(mode) = permission_mode {
        if !PERMISSION_MODES.contains(&mode) {
            bail!("unknown permission mode: {mode}");
        }
    }
    Ok(())
}

pub(super) fn launch(
    req: &StartAgent,
    session_id: &str,
    permission_mode: Option<&str>,
    config_dir: Option<&Path>,
) -> Launch {
    let projects = config_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(workbench_core::paths::claude_user_dir)
        .join("projects");
    let history = claude_transcript::find_transcript(&projects, session_id);
    let transcript = history.as_deref().map(Transcript::load).unwrap_or_default();

    let mut cmd = super::session::base_command(claude_accounts::claude_binary(), req);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--replay-user-messages",
        // Undocumented but what the Agent SDK passes: permission prompts
        // arrive as `can_use_tool` control requests on stdout.
        "--permission-prompt-tool",
        "stdio",
    ]);
    if let Some(mode) = permission_mode {
        cmd.args(["--permission-mode", mode]);
    }
    let id_flag = if history.is_some() {
        "--resume"
    } else {
        "--session-id"
    };
    cmd.args([id_flag, session_id]);
    if let Some(dir) = config_dir {
        cmd.env(claude_accounts::CONFIG_DIR_ENV, dir);
    }
    Launch {
        cmd,
        driver: Driver::Claude(transcript),
        // The SDK handshake: without it the CLI won't route permission prompts here.
        hello: vec![control(json!({"subtype": "initialize"}))],
        ready: Some(session_id.to_string()),
        program: "claude",
    }
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
        send: applied.reply.into_iter().collect(),
        items: applied.items,
        meta: applied.meta,
        commands: applied.commands,
        new_id: applied.new_session_id,
        ..Effects::default()
    }
}

pub(super) fn prompt(t: &mut Transcript, text: &str, images: &[PromptImage]) -> Result<Effects> {
    let content = if images.is_empty() {
        json!(text)
    } else {
        let mut blocks: Vec<Value> = images
            .iter()
            .map(|img| {
                json!({"type": "image", "source": {
                    "type": "base64", "media_type": img.media_type, "data": img.data,
                }})
            })
            .collect();
        if !text.trim().is_empty() {
            blocks.push(json!({"type": "text", "text": text}));
        }
        Value::Array(blocks)
    };
    t.set_busy();
    Ok(Effects {
        send: vec![json!({
            "type": "user",
            "message": {"role": "user", "content": content},
            "parent_tool_use_id": null,
            // Hosts relaying typed input must say so; unattributed input fails
            // closed at the CLI's isHuman() trust gates.
            "origin": {"kind": "human"},
        })],
        meta: true,
        ..Effects::default()
    })
}

pub(super) fn approve(
    t: &mut Transcript,
    request_id: &str,
    decision: ApprovalDecision,
    answers: Option<&Map<String, Value>>,
) -> Result<Effects> {
    // `None`: already answered (another device, or twice).
    Ok(match t.resolve_approval(request_id, decision, answers) {
        Some((i, response)) => Effects {
            send: vec![response],
            items: vec![i],
            ..Effects::default()
        },
        None => Effects::default(),
    })
}

pub(super) fn interrupt() -> Effects {
    Effects {
        send: vec![control(json!({"subtype": "interrupt"}))],
        ..Effects::default()
    }
}

pub(super) fn set_mode(t: &mut Transcript, mode: &str) -> Result<Effects> {
    if !PERMISSION_MODES.contains(&mode) {
        bail!("unknown permission mode: {mode}");
    }
    t.set_permission_mode(mode);
    Ok(changed_meta(control(
        json!({"subtype": "set_permission_mode", "mode": mode}),
    )))
}

pub(super) fn set_model(t: &mut Transcript, model: &str) -> Result<Effects> {
    let valid = !model.is_empty()
        && model.len() <= 80
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._[]".contains(&b));
    if !valid {
        bail!("unknown model: {model}");
    }
    t.set_model_choice(model);
    Ok(changed_meta(control(
        json!({"subtype": "set_model", "model": model}),
    )))
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
