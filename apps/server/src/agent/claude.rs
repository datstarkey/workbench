//! The Claude driver: `claude -p` speaking stream-json, events folded by
//! [`Transcript`]. The session id is the Claude session id, so the same
//! conversation can move between chat and a terminal running
//! `claude --resume <id>` — one process at a time.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use workbench_core::chat_attachment::PDF_TYPE;
use workbench_core::claude_accounts;
use workbench_core::claude_launch::PERMISSION_MODES;
use workbench_core::claude_transcript::{self, ApprovalDecision, Transcript, TranscriptMeta};

use super::driver::{Driver, Effects, Launch};
use super::{PromptFile, PromptImage, StartAgent};

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

/// The session's JSONL, once the CLI has written one.
pub(super) fn history(config_dir: Option<&Path>, session_id: &str) -> Option<PathBuf> {
    let projects = config_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(workbench_core::paths::claude_user_dir)
        .join("projects");
    claude_transcript::find_transcript(&projects, session_id)
}

/// Whether the account turned Claude in Chrome on by default. An interactive
/// `claude` honours `claudeInChromeDefaultEnabled`; `-p` ignores it and only
/// loads the extension's MCP server with `--chrome` (verified on 2.1.286).
fn chrome_enabled(config_dir: Option<&Path>) -> bool {
    let path = config_dir.map_or_else(
        || workbench_core::paths::home_dir().join(".claude.json"),
        |dir| dir.join(".claude.json"),
    );
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|v| v.get("claudeInChromeDefaultEnabled")?.as_bool())
        .unwrap_or(false)
}

/// A driver holding the session's history, for a terminal's `claude` the
/// plugin feeds (no process is started).
pub(super) fn history_driver(
    config_dir: Option<&Path>,
    session_id: &str,
    resume_at: Option<&str>,
) -> Driver {
    let transcript = history(config_dir, session_id)
        .as_deref()
        .map(|path| Transcript::load_at(path, resume_at))
        .unwrap_or_default();
    Driver::Claude(transcript)
}

/// `resume_at`: continue the conversation from this entry, dropping what
/// came after it (a rewind).
pub(super) fn launch(
    req: &StartAgent,
    session_id: &str,
    permission_mode: Option<&str>,
    config_dir: Option<&Path>,
    resume_at: Option<&str>,
) -> Launch {
    let history = history(config_dir, session_id);
    let transcript = history
        .as_deref()
        .map(|path| Transcript::load_at(path, resume_at))
        .unwrap_or_default();

    let mut cmd = super::session::base_command(claude_accounts::claude_binary(), req);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        // Failed and blocking hooks become chat notices (successful ones are dropped).
        "--include-hook-events",
        "--replay-user-messages",
        // Undocumented but what the Agent SDK passes: permission prompts
        // arrive as `can_use_tool` control requests on stdout.
        "--permission-prompt-tool",
        "stdio",
    ]);
    if let Some(mode) = permission_mode {
        cmd.args(["--permission-mode", mode]);
    }
    if chrome_enabled(config_dir) {
        cmd.arg("--chrome");
    }
    let id_flag = if history.is_some() {
        "--resume"
    } else {
        "--session-id"
    };
    cmd.args([id_flag, session_id]);
    if let Some(at) = resume_at.filter(|_| history.is_some()) {
        cmd.arg(format!("--resume-session-at={at}"));
    }
    // What the Agent SDK sets for `enableFileCheckpointing`: edits are backed
    // up per prompt, so `rewind_files` can restore them.
    cmd.env("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING", "true");
    if let Some(dir) = config_dir {
        cmd.env(claude_accounts::CONFIG_DIR_ENV, dir);
    }
    Launch {
        cmd,
        driver: Driver::Claude(transcript),
        // The SDK handshake: without it the CLI won't route permission prompts here.
        // `promptSuggestions`: a likely next prompt after each turn, shown as a chip.
        hello: vec![control(
            json!({"subtype": "initialize", "promptSuggestions": true}),
        )],
        ready: Some(session_id.to_string()),
        program: "claude",
    }
}

/// Keep the model and effort picked in chat across a relaunch: the new
/// process asks for them right after the handshake.
pub(super) fn carry_over(launch: &mut Launch, meta: &TranscriptMeta) {
    let Driver::Claude(t) = &mut launch.driver else {
        return;
    };
    let model = meta
        .model_choice
        .as_deref()
        .and_then(|m| set_model(t, m, false).ok());
    let effort = meta.effort.as_deref().and_then(|e| set_effort(t, e).ok());
    launch
        .hello
        .extend(model.into_iter().chain(effort).flat_map(|e| e.send));
}

/// The SDK handshake; its reply lists the models the chat's picker offers.
pub(super) fn hello() -> Value {
    control(json!({"subtype": "initialize", "promptSuggestions": true}))
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
        response: applied.response,
        ..Effects::default()
    }
}

pub(super) fn prompt(
    t: &mut Transcript,
    text: &str,
    images: &[PromptImage],
    files: &[PromptFile],
) -> Result<Effects> {
    let content = if images.is_empty() && files.is_empty() {
        json!(text)
    } else {
        let images = images.iter().map(|img| {
            json!({"type": "image", "source": {
                "type": "base64", "media_type": img.media_type, "data": img.data,
            }})
        });
        // Verified with CLI 2.1.286: document blocks pass through stream-json
        // input, base64 for PDFs and a text source for text files.
        let files = files.iter().map(|f| {
            let kind = if f.media_type == PDF_TYPE {
                "base64"
            } else {
                "text"
            };
            json!({"type": "document", "title": f.name, "source": {
                "type": kind, "media_type": f.media_type, "data": f.data,
            }})
        });
        let mut blocks: Vec<Value> = files.chain(images).collect();
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
            // File checkpoints are keyed by this id; the CLI echoes it back.
            "uuid": uuid::Uuid::new_v4().to_string(),
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

pub(super) fn rewind_files(message_id: &str, dry_run: bool) -> Result<(String, Effects)> {
    if !claude_transcript::is_uuid(message_id) {
        bail!("message id must be a UUID");
    }
    let msg = control(json!({
        "subtype": "rewind_files", "user_message_id": message_id, "dry_run": dry_run,
    }));
    let id = msg["request_id"].as_str().unwrap_or_default().to_string();
    Ok((
        id,
        Effects {
            send: vec![msg],
            ..Effects::default()
        },
    ))
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

/// `persist`: also make it the default (a terminal session's `/config`), not
/// just this session's.
pub(super) fn set_model(t: &mut Transcript, model: &str, persist: bool) -> Result<Effects> {
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
        json!({"subtype": "set_model", "model": model, "persist": persist}),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_follows_the_accounts_default() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!chrome_enabled(Some(dir.path())));
        let json = dir.path().join(".claude.json");
        std::fs::write(&json, r#"{"claudeInChromeDefaultEnabled":true}"#).unwrap();
        assert!(chrome_enabled(Some(dir.path())));
        std::fs::write(&json, r#"{"claudeInChromeDefaultEnabled":false}"#).unwrap();
        assert!(!chrome_enabled(Some(dir.path())));
    }

    #[test]
    fn attachments_go_before_the_text_as_content_blocks() {
        let mut t = Transcript::default();
        let image = PromptImage {
            media_type: "image/png".into(),
            data: "iVBORw==".into(),
        };
        let files = [
            PromptFile {
                name: "report.pdf".into(),
                media_type: PDF_TYPE.into(),
                data: "JVBERg==".into(),
            },
            PromptFile {
                name: "main.rs".into(),
                media_type: "text/plain".into(),
                data: "fn main() {}".into(),
            },
        ];
        let fx = prompt(&mut t, "look", &[image], &files).unwrap();
        let content = &fx.send[0]["message"]["content"];
        assert_eq!(content[0]["type"], "document");
        assert_eq!(content[0]["title"], "report.pdf");
        assert_eq!(content[0]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["type"], "text");
        assert_eq!(content[1]["source"]["data"], "fn main() {}");
        assert_eq!(content[2]["type"], "image");
        assert_eq!(content[3], json!({"type": "text", "text": "look"}));

        let plain = prompt(&mut t, "hi", &[], &[]).unwrap();
        assert_eq!(plain.send[0]["message"]["content"], "hi");
    }

    const MSG: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn rewind_files_is_the_sdks_control_request() {
        let (id, effects) = rewind_files(MSG, true).unwrap();
        let [msg] = effects.send.as_slice() else {
            panic!("one line");
        };
        assert_eq!(msg["type"], "control_request");
        assert_eq!(msg["request_id"], id.as_str());
        assert_eq!(
            msg["request"],
            json!({"subtype": "rewind_files", "user_message_id": MSG, "dry_run": true})
        );
        assert!(rewind_files("../etc", false).is_err());
    }

    #[test]
    fn prompts_carry_the_id_checkpoints_are_keyed_by() {
        let effects = prompt(&mut Transcript::default(), "hi", &[], &[]).unwrap();
        let id = effects.send[0]["uuid"].as_str().unwrap();
        assert!(claude_transcript::is_uuid(id));
    }

    #[test]
    fn a_relaunch_keeps_the_chosen_model_and_effort() {
        let mut launch = Launch {
            cmd: std::process::Command::new("true"),
            driver: Driver::Claude(Transcript::default()),
            hello: vec![],
            ready: None,
            program: "claude",
        };
        let meta = TranscriptMeta {
            model_choice: Some("opus".into()),
            effort: Some("high".into()),
            ..TranscriptMeta::default()
        };
        carry_over(&mut launch, &meta);
        let subtypes: Vec<_> = launch
            .hello
            .iter()
            .map(|m| m["request"]["subtype"].clone())
            .collect();
        assert_eq!(subtypes, [json!("set_model"), json!("apply_flag_settings")]);
    }
}
