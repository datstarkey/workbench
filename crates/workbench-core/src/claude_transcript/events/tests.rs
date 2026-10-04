use serde_json::{json, Value};

use crate::claude_transcript::{ArtifactInfo, ChatView, EventKind, Transcript, TranscriptItem};

fn system(subtype: &str, uuid: &str, rest: Value) -> Value {
    let mut obj = json!({"type": "system", "subtype": subtype, "uuid": uuid, "session_id": "s"});
    obj.as_object_mut()
        .unwrap()
        .extend(rest.as_object().unwrap().clone());
    obj
}

fn attachment(uuid: &str, att: Value) -> Value {
    json!({"type": "attachment", "uuid": uuid, "attachment": att})
}

/// `(kind, title, detail, files)` of every event item.
fn events(t: &Transcript) -> Vec<(EventKind, String, Option<String>, Vec<String>)> {
    t.items()
        .iter()
        .filter_map(|i| match i {
            TranscriptItem::Event {
                event,
                title,
                detail,
                files,
                ..
            } => Some((*event, title.clone(), detail.clone(), files.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn auto_mode_denials_name_the_tool_and_reason() {
    let mut t = Transcript::default();
    let applied = t.apply(&system(
        "permission_denied",
        "d1",
        json!({"tool_name": "Bash", "tool_use_id": "toolu_1", "decision_reason_type": "classifier",
            "decision_reason": "Deletes files outside the project", "message": "Denied"}),
    ));
    assert_eq!(applied.unknown_kind, None);
    assert_eq!(applied.items, vec![0]);
    assert_eq!(
        events(&t),
        vec![(
            EventKind::PermissionDenied,
            "Auto mode blocked Bash".into(),
            Some("Deletes files outside the project".into()),
            vec![]
        )]
    );
}

#[test]
fn a_denial_without_a_reason_shows_the_message() {
    let mut t = Transcript::default();
    t.apply(&system(
        "permission_denied",
        "d1",
        json!({"tool_name": "Write", "tool_use_id": "toolu_1", "decision_reason_type": "rule",
            "message": "Write to .env is denied"}),
    ));
    let (_, title, detail, _) = &events(&t)[0];
    assert_eq!(title, "A permission rule blocked Write");
    assert_eq!(detail.as_deref(), Some("Write to .env is denied"));
}

#[test]
fn only_hooks_that_fail_block_or_speak_are_shown() {
    let mut t = Transcript::default();
    let hook = |uuid: &str, outcome: &str, exit: i64, stdout: &str, stderr: &str| {
        system(
            "hook_response",
            uuid,
            json!({"hook_id": uuid, "hook_name": "PreToolUse:Bash", "hook_event": "PreToolUse",
                "output": "", "stdout": stdout, "stderr": stderr, "exit_code": exit, "outcome": outcome}),
        )
    };
    t.apply(&hook("h1", "success", 0, "", ""));
    t.apply(&hook("h2", "success", 0, r#"{"suppressOutput":true}"#, ""));
    t.apply(&hook("h3", "cancelled", 0, "", ""));
    assert!(
        events(&t).is_empty(),
        "successful and cancelled hooks stay quiet"
    );

    t.apply(&hook("h4", "error", 2, "", "rm -rf is not allowed"));
    t.apply(&hook("h5", "error", 1, "", "node: command not found"));
    t.apply(&hook(
        "h6",
        "success",
        0,
        r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"Use the task runner"}}"#,
        "",
    ));
    t.apply(&hook(
        "h7",
        "success",
        0,
        r#"{"systemMessage":"Formatted 3 files"}"#,
        "",
    ));
    let shown: Vec<(String, Option<String>)> = events(&t)
        .into_iter()
        .map(|(kind, title, detail, _)| {
            assert_eq!(kind, EventKind::Hook);
            (title, detail)
        })
        .collect();
    assert_eq!(
        shown,
        vec![
            (
                "PreToolUse:Bash hook blocked".into(),
                Some("rm -rf is not allowed".into())
            ),
            (
                "PreToolUse:Bash hook failed (exit 1)".into(),
                Some("node: command not found".into())
            ),
            (
                "PreToolUse:Bash hook blocked".into(),
                Some("Use the task runner".into())
            ),
            (
                "PreToolUse:Bash hook".into(),
                Some("Formatted 3 files".into())
            ),
        ]
    );
}

#[test]
fn jsonl_hook_attachments_match_the_live_notices() {
    let mut t = Transcript::default();
    t.apply(&attachment(
        "a1",
        json!({"type": "hook_success", "hookName": "Stop", "hookEvent": "Stop", "content": ""}),
    ));
    t.apply(&attachment(
        "a2",
        json!({"type": "hook_cancelled", "hookName": "Stop", "hookEvent": "Stop"}),
    ));
    t.apply(&attachment(
        "a3",
        json!({"type": "hook_non_blocking_error", "hookName": "Stop", "hookEvent": "Stop",
            "stderr": "Plugin directory does not exist", "stdout": "", "exitCode": 1}),
    ));
    t.apply(&attachment(
        "a4",
        json!({"type": "hook_blocking_error", "hookName": "PreToolUse:Bash", "hookEvent": "PreToolUse",
            "blockingError": {"blockingError": "rm -rf is not allowed", "command": "guard.sh"}}),
    ));
    t.apply(&system(
        "stop_hook_summary",
        "s1",
        json!({"hookCount": 1, "hookErrors": [], "preventedContinuation": false, "stopReason": ""}),
    ));
    t.apply(&system(
        "stop_hook_summary",
        "s2",
        json!({"hookCount": 1, "hookErrors": [], "preventedContinuation": true, "stopReason": "Tests are failing"}),
    ));
    let titles: Vec<(String, Option<String>)> = events(&t)
        .into_iter()
        .map(|(_, title, detail, _)| (title, detail))
        .collect();
    assert_eq!(
        titles,
        vec![
            (
                "Stop hook failed (exit 1)".into(),
                Some("Plugin directory does not exist".into())
            ),
            (
                "PreToolUse:Bash hook blocked".into(),
                Some("rm -rf is not allowed".into())
            ),
            (
                "Stop hook stopped Claude".into(),
                Some("Tests are failing".into())
            ),
        ]
    );
}

#[test]
fn queued_prompts_still_fold_alongside_hook_attachments() {
    let mut t = Transcript::default();
    t.apply(&attachment(
        "q1",
        json!({"type": "queued_command", "commandMode": "prompt", "prompt": "and the docs"}),
    ));
    assert!(matches!(&t.items()[0], TranscriptItem::User { text, .. } if text == "and the docs"));
    assert!(events(&t).is_empty());
}

#[test]
fn recalled_memories_list_their_files_from_either_source() {
    let mut live = Transcript::default();
    live.apply(&system(
        "memory_recall",
        "m1",
        json!({"mode": "select", "memories": [
            {"path": "/home/u/.claude/memory/style.md", "scope": "personal"},
            {"path": "/home/u/.claude/memory/release.md", "scope": "personal"}
        ]}),
    ));
    let mut history = Transcript::default();
    history.apply(&attachment(
        "m1",
        json!({"type": "relevant_memories", "memories": [
            {"path": "/home/u/.claude/memory/style.md", "content": "Tabs."},
            {"path": "/home/u/.claude/memory/release.md", "content": "Tag first."}
        ]}),
    ));
    for t in [&live, &history] {
        let (kind, title, detail, files) = &events(t)[0];
        assert_eq!(*kind, EventKind::Memory);
        assert_eq!(title, "Recalled 2 memories");
        assert_eq!(*detail, None);
        assert_eq!(files.len(), 2);
    }
}

#[test]
fn a_synthesised_memory_shows_its_text() {
    let mut t = Transcript::default();
    t.apply(&system(
        "memory_recall",
        "m1",
        json!({"mode": "synthesize", "memories": [
            {"path": "<synthesis:/home/u/.claude/memory>", "scope": "personal", "content": "Prefers bun."}
        ]}),
    ));
    let (_, title, detail, files) = &events(&t)[0];
    assert_eq!(title, "Recalled from memory");
    assert_eq!(detail.as_deref(), Some("Prefers bun."));
    assert!(files.is_empty());
}

#[test]
fn refusals_say_what_happened() {
    let mut t = Transcript::default();
    t.apply(&system(
        "model_refusal_fallback",
        "r1",
        json!({"trigger": "refusal", "direction": "retry", "scope": "session",
            "original_model": "claude-opus-5-5", "fallback_model": "claude-sonnet-5",
            "request_id": null, "api_refusal_explanation": "Flagged as cyber", "content": "Retrying"}),
    ));
    t.apply(&system(
        "model_refusal_no_fallback",
        "r2",
        json!({"original_model": "claude-opus-5-5", "request_id": null, "content": "The model declined."}),
    ));
    let got: Vec<(EventKind, String, Option<String>)> = events(&t)
        .into_iter()
        .map(|(k, title, detail, _)| (k, title, detail))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                EventKind::Refusal,
                "claude-opus-5-5 refused, retried on claude-sonnet-5".into(),
                Some("Flagged as cyber".into())
            ),
            (
                EventKind::Refusal,
                "claude-opus-5-5 refused this request".into(),
                Some("The model declined.".into())
            ),
        ]
    );
}

#[test]
fn prompt_suggestions_last_until_the_next_turn() {
    let mut t = Transcript::default();
    let applied = t.apply(&json!({"type": "prompt_suggestion", "suggestion": "Run the tests", "uuid": "p1", "session_id": "s"}));
    assert_eq!(applied.unknown_kind, None);
    assert!(applied.meta);
    assert_eq!(t.meta().prompt_suggestion.as_deref(), Some("Run the tests"));

    t.apply(&json!({"type": "user", "uuid": "u1", "message": {"role": "user", "content": "Run the tests"}}));
    assert_eq!(t.meta().prompt_suggestion, None);

    t.apply(&json!({"type": "result", "subtype": "success"}));
    t.apply(&json!({"type": "prompt_suggestion", "suggestion": "Commit it", "uuid": "p2"}));
    t.set_busy();
    assert_eq!(t.meta().prompt_suggestion, None);
}

fn artifact_call(
    t: &mut Transcript,
    id: &str,
    input: Value,
    result: Value,
    text: &str,
    error: bool,
) {
    t.apply(
        &json!({"type": "assistant", "uuid": format!("a-{id}"), "message": {"id": format!("m-{id}"),
        "content": [{"type": "tool_use", "id": id, "name": "Artifact", "input": input}]}}),
    );
    t.apply(
        &json!({"type": "user", "uuid": format!("r-{id}"), "toolUseResult": result,
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id,
            "content": text, "is_error": error}]}}),
    );
}

#[test]
fn artifact_calls_record_their_links() {
    let mut t = Transcript::default();
    artifact_call(
        &mut t,
        "t1",
        json!({"action": "quickstart", "intent": "document"}),
        json!({"quickstart": {"intent": "document"}}),
        "Quickstart for a document.",
        false,
    );
    artifact_call(
        &mut t,
        "t2",
        json!({"title": "Release plan", "type_url": "https://claude.ai/artifact/type1"}),
        json!({"created_from_type": true, "url": "https://claude.ai/artifact/abc", "version": "v1", "title": "Release plan"}),
        "Created a new Artifact at https://claude.ai/artifact/abc",
        false,
    );
    artifact_call(
        &mut t,
        "t3",
        json!({"url": "https://claude.ai/artifact/abc", "file_path": "/tmp/plan.md"}),
        json!({"url": "https://claude.ai/artifact/abc", "updated": true, "version": "v2", "title": "Release plan"}),
        "Updated the Artifact at https://claude.ai/artifact/abc (Version 2)",
        false,
    );
    artifact_call(
        &mut t,
        "t4",
        json!({"url": "javascript:alert(1)"}),
        json!({"url": "javascript:alert(1)", "updated": true}),
        "Updated",
        false,
    );
    artifact_call(
        &mut t,
        "t5",
        json!({"url": "https://claude.ai/artifact/zzz"}),
        json!("Error: refused"),
        "refused",
        true,
    );
    let got: Vec<(&str, &str, &str)> = t
        .meta()
        .artifacts
        .iter()
        .map(|a: &ArtifactInfo| (a.tool_use_id.as_str(), a.url.as_str(), a.action.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("t2", "https://claude.ai/artifact/abc", "created"),
            ("t3", "https://claude.ai/artifact/abc", "updated"),
        ]
    );
    assert_eq!(t.meta().artifacts[1].version.as_deref(), Some("v2"));

    // A replayed result doesn't list the call twice; /clear forgets them.
    t.apply(&json!({"type": "user", "uuid": "r-t3b", "toolUseResult": {"url": "https://claude.ai/artifact/abc", "updated": true},
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t3", "content": "Updated"}]}}));
    assert_eq!(t.meta().artifacts.len(), 2);
    t.apply(&json!({"type": "conversation_reset", "new_conversation_id": "n"}));
    assert!(t.meta().artifacts.is_empty());
}

#[test]
fn a_rewind_forgets_artifacts_from_the_abandoned_branch() {
    let line = |v: Value| v.to_string() + "\n";
    let prompt = |uuid: &str, parent: Option<&str>, text: &str| {
        line(json!({"type": "user", "uuid": uuid, "parentUuid": parent,
            "message": {"role": "user", "content": text}}))
    };
    let mut jsonl = prompt("p0", None, "Start");
    jsonl += &prompt("u1", Some("p0"), "Publish the plan");
    jsonl += &line(
        json!({"type": "assistant", "uuid": "a1", "parentUuid": "u1",
        "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Artifact", "input": {}}]}}),
    );
    jsonl += &line(json!({"type": "user", "uuid": "r1", "parentUuid": "a1",
        "toolUseResult": {"url": "https://claude.ai/artifact/abc", "created_from_type": true},
        "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "Created"}]}}));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(&path, &jsonl).unwrap();
    assert_eq!(Transcript::load_at(&path, None).meta().artifacts.len(), 1);

    // Rewound to before "Publish the plan" and continued with another prompt.
    jsonl += &prompt("u2", Some("p0"), "Never mind");
    std::fs::write(&path, &jsonl).unwrap();
    let t = Transcript::load_at(&path, None);
    assert!(t.meta().artifacts.is_empty());
    // Suggestions live only in the stream, so a relaunched session starts without one.
    assert_eq!(t.meta().prompt_suggestion, None);
}
