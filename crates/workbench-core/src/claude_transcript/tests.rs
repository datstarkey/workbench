use super::parse::MAX_TEXT_BYTES;
use super::*;
use serde_json::json;
use std::fs;

const SID: &str = "7b3c54f4-ba22-4654-9af9-037d1cd8e555";

fn user(uuid: &str, content: Value) -> Value {
    json!({"type":"user","uuid":uuid,"timestamp":"2026-09-30T21:07:10Z","message":{"role":"user","content":content}})
}

fn assistant(uuid: &str, msg_id: &str, block: Value) -> Value {
    json!({"type":"assistant","uuid":uuid,"message":{"id":msg_id,"model":"claude-opus-5-5","content":[block],
        "usage":{"input_tokens":2,"cache_read_input_tokens":100,"cache_creation_input_tokens":50}}})
}

fn stream(event: Value) -> Value {
    json!({"type":"stream_event","event":event,"parent_tool_use_id":null})
}

fn user_texts(t: &Transcript) -> Vec<&str> {
    t.items()
        .iter()
        .filter_map(|i| match i {
            TranscriptItem::User { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn title_falls_back_to_the_first_real_prompt_live_and_on_resume() {
    let lines = [
        json!({"type":"user","isMeta":true,"message":{"content":"Injected context"}}),
        user("command", json!("<command-name>/model</command-name>")),
        user("first", json!("Fix the keyboard inset\nDetails below")),
        user("second", json!("Now add tests")),
    ];
    let mut live = Transcript::default();
    for line in &lines[..2] {
        live.apply(line);
    }
    assert_eq!(live.meta().title, None);
    assert!(live.apply(&lines[2]).meta);
    live.apply(&lines[3]);
    assert_eq!(live.meta().title.as_deref(), Some("Fix the keyboard inset"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    fs::write(&path, lines.map(|l| l.to_string()).join("\n")).unwrap();
    let resumed = Transcript::load(&path);
    assert_eq!(resumed.meta().title, live.meta().title);
    assert!(!resumed.meta().busy);

    live.apply(&json!({"type":"conversation_reset","new_conversation_id":SID}));
    assert_eq!(live.meta().title, None);
    live.apply(&user("new", json!("A different task")));
    assert_eq!(live.meta().title.as_deref(), Some("A different task"));
}

#[test]
fn resume_restores_the_latest_saved_title_and_preserves_renames() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    let lines = [
        user("u", json!("The original prompt")),
        json!({"type":"ai-title","aiTitle":"First generated title","sessionId":SID}),
        json!({"type":"ai-title","aiTitle":"Updated generated title","sessionId":SID}),
    ];
    let mut contents = lines.map(|l| l.to_string()).join("\n");
    fs::write(&path, &contents).unwrap();
    let mut resumed = Transcript::load(&path);
    assert_eq!(
        resumed.meta().title.as_deref(),
        Some("Updated generated title")
    );
    let rename = json!({"type":"custom-title","customTitle":"My session name","sessionId":SID});
    assert!(resumed.apply(&rename).meta);
    contents.push_str(&format!("\n{rename}"));
    fs::write(&path, contents).unwrap();
    let mut resumed = Transcript::load(&path);
    for line in [
        json!({"type":"ai-title","aiTitle":"Later generated title"}),
        json!({"type":"custom-title","customTitle":" "}),
        json!({"type":"ai-title"}),
        json!({"type":"custom-title","customTitle":"Subagent name","isSidechain":true}),
    ] {
        resumed.apply(&line);
    }
    assert_eq!(resumed.meta().title.as_deref(), Some("My session name"));
    resumed.apply(&json!({"type":"conversation_reset","new_conversation_id":SID}));
    resumed.apply(&json!({"type":"ai-title","aiTitle":"New conversation"}));
    assert_eq!(resumed.meta().title.as_deref(), Some("New conversation"));
}

#[test]
fn folds_a_jsonl_turn_into_chat_items() {
    let mut t = Transcript::default();
    t.apply(&user("u1", json!("Fix the keyboard inset")));
    assert!(t.meta().busy);
    t.apply(&assistant(
        "a1",
        "m1",
        json!({"type":"text","text":"On it."}),
    ));
    t.apply(&assistant(
        "a2",
        "m1",
        json!({"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"bun test"}}),
    ));
    let applied = t.apply(&json!({"type":"user","uuid":"u2","message":{"content":[
        {"type":"tool_result","tool_use_id":"toolu_1","content":"41 passed","is_error":false}]}}));
    assert_eq!(applied.items, vec![2]);
    t.apply(&json!({"type":"system","subtype":"turn_duration","durationMs":10}));

    assert_eq!(t.items().len(), 3);
    assert!(matches!(&t.items()[1], TranscriptItem::Text { text, .. } if text == "On it."));
    assert!(
        matches!(&t.items()[2], TranscriptItem::Tool { status: ToolStatus::Ok, output: Some(o), .. } if o == "41 passed")
    );
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(t.meta().context_tokens, Some(152));
    assert!(!t.meta().busy);
}

#[test]
fn edit_results_carry_the_patch_in_either_spelling() {
    for key in ["toolUseResult", "tool_use_result"] {
        let mut t = Transcript::default();
        t.apply(&assistant(
            "a1",
            "m1",
            json!({"type":"tool_use","id":"toolu_e","name":"Edit","input":{"file_path":"/x.ts"}}),
        ));
        let mut line = json!({"type":"user","uuid":"u","message":{"content":[
            {"type":"tool_result","tool_use_id":"toolu_e","content":"ok"}]}});
        line[key] = json!({"structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-a","+b"]}]});
        t.apply(&line);
        let TranscriptItem::Tool { patch: Some(p), .. } = &t.items()[0] else {
            panic!("expected patch for {key}");
        };
        assert_eq!(p[0]["lines"], json!(["-a", "+b"]));
    }
}

#[test]
fn streamed_text_is_replaced_in_place_by_the_final_block() {
    let mut t = Transcript::default();
    t.apply(&stream(
        json!({"type":"message_start","message":{"id":"m1","model":"claude-opus-5-5"}}),
    ));
    assert!(t.meta().busy);
    t.apply(&stream(
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
    ));
    t.apply(&stream(
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}),
    ));
    let a = t.apply(&stream(
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}),
    ));
    assert_eq!(a.items, vec![0]);
    assert!(
        matches!(&t.items()[0], TranscriptItem::Text { id, text } if id == "m1:0" && text == "Hello")
    );

    t.apply(&assistant(
        "a1",
        "m1",
        json!({"type":"text","text":"Hello there."}),
    ));
    assert_eq!(t.items().len(), 1, "final block must not add a second item");
    assert!(
        matches!(&t.items()[0], TranscriptItem::Text { id, text } if id == "m1:0" && text == "Hello there.")
    );

    t.apply(&json!({"type":"result","subtype":"success","is_error":false}));
    assert!(!t.meta().busy);
}

#[test]
fn a_streamed_tool_call_shows_running_then_gets_its_input() {
    let mut t = Transcript::default();
    t.apply(&stream(
        json!({"type":"message_start","message":{"id":"m1"}}),
    ));
    t.apply(&stream(json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":"toolu_9","name":"Bash","input":{}}})));
    assert!(
        matches!(&t.items()[0], TranscriptItem::Tool { name, status: ToolStatus::Running, .. } if name == "Bash")
    );
    t.apply(&assistant(
        "a1",
        "m1",
        json!({"type":"tool_use","id":"toolu_9","name":"Bash","input":{"command":"echo hi"}}),
    ));
    assert_eq!(t.items().len(), 1);
    assert!(
        matches!(&t.items()[0], TranscriptItem::Tool { input, .. } if input["command"] == "echo hi")
    );
}

#[test]
fn approvals_round_trip_to_control_responses() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"control_request","request_id":"req-1","request":{
        "subtype":"can_use_tool","tool_name":"Bash","input":{"command":"touch x"},
        "description":"Create x","blocked_path":"/repo/x",
        "permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch x"}],"behavior":"allow","destination":"localSettings"}]}}));
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Approval { tool, can_always_allow: true, decision: None, .. } if tool == "Bash"
    ));
    assert_eq!(t.pending_approval_ids(), vec!["req-1".to_string()]);

    let (i, response) = t
        .resolve_approval("req-1", ApprovalDecision::AlwaysAllow, None)
        .expect("pending approval");
    assert_eq!(i, 0);
    assert_eq!(response["type"], "control_response");
    assert_eq!(response["response"]["request_id"], "req-1");
    let body = &response["response"]["response"];
    assert_eq!(body["behavior"], "allow");
    assert_eq!(body["updatedInput"]["command"], "touch x");
    assert_eq!(body["updatedPermissions"][0]["type"], "addRules");
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Approval {
            decision: Some(ApprovalDecision::AlwaysAllow),
            ..
        }
    ));
    assert!(
        t.resolve_approval("req-1", ApprovalDecision::Deny, None)
            .is_none(),
        "answered once"
    );
}

#[test]
fn waiting_on_is_the_oldest_unanswered_approval() {
    let ask = |id: &str| {
        json!({"type":"control_request","request_id":id,"request":{
            "subtype":"can_use_tool","tool_name":"Bash","input":{}}})
    };
    let mut t = Transcript::default();
    assert!(t.waiting_on().is_none());
    for id in ["a", "b", "c"] {
        t.apply(&ask(id));
    }
    assert_eq!(t.waiting_on().map(TranscriptItem::id), Some("a"));
    t.resolve_approval("a", ApprovalDecision::Allow, None);
    assert_eq!(t.waiting_on().map(TranscriptItem::id), Some("b"));
    t.apply(&json!({"type":"control_cancel_request","request_id":"b"}));
    assert_eq!(t.waiting_on().map(TranscriptItem::id), Some("c"));
    t.resolve_approval("c", ApprovalDecision::Deny, None);
    assert!(t.waiting_on().is_none());
}

#[test]
fn an_orphaned_approval_expires_with_its_calls_result_or_the_turn() {
    let ask = |id: &str, tool: &str| {
        json!({"type":"control_request","request_id":id,"request":{
            "subtype":"can_use_tool","tool_name":"Bash","input":{},"tool_use_id":tool}})
    };
    let expired = |t: &Transcript, id: &str| {
        matches!(
            &t.items()[t.index[id]],
            TranscriptItem::Approval { expired: true, .. }
        )
    };
    let mut t = Transcript::default();
    t.apply(&ask("a", "toolu_a"));
    t.apply(&ask("b", "toolu_b"));
    let applied = t.apply(&user(
        "u1",
        json!([{"type":"tool_result","tool_use_id":"toolu_a","content":"ok"}]),
    ));
    assert!(
        expired(&t, "a"),
        "its call has a result: nobody waits on it"
    );
    assert!(applied.items.contains(&t.index["a"]));
    assert!(t
        .resolve_approval("a", ApprovalDecision::Allow, None)
        .is_none());
    assert_eq!(t.waiting_on().map(TranscriptItem::id), Some("b"));

    t.apply(&json!({"type":"result","subtype":"success"}));
    assert!(expired(&t, "b"), "the turn ended");
    assert!(t.waiting_on().is_none());
}

#[test]
fn running_tool_is_the_newest_unfinished_call_of_a_live_turn() {
    let tool_use = |id: &str| {
        assistant(
            &format!("a-{id}"),
            &format!("m-{id}"),
            json!({"type":"tool_use","id":id,"name":"Bash","input":{"command":"ls"}}),
        )
    };
    let tool_result = |id: &str| {
        user(
            &format!("u-{id}"),
            json!([{"type":"tool_result","tool_use_id":id,"content":"ok"}]),
        )
    };
    let mut t = Transcript::default();
    t.apply(&user("u0", json!("go")));
    t.apply(&tool_use("t1"));
    t.apply(&tool_use("t2"));
    assert_eq!(t.running_tool().map(TranscriptItem::id), Some("t2"));
    t.apply(&tool_result("t2"));
    assert_eq!(t.running_tool().map(TranscriptItem::id), Some("t1"));

    t.apply(&json!({"type":"result","subtype":"success","is_error":false}));
    assert!(
        t.running_tool().is_none(),
        "an idle session runs nothing, even with a call left unfinished"
    );
}

#[test]
fn deny_tells_claude_why() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Write","input":{}}}));
    let (_, response) = t
        .resolve_approval("r", ApprovalDecision::Deny, None)
        .unwrap();
    assert_eq!(response["response"]["response"]["behavior"], "deny");
    assert!(response["response"]["response"]["message"]
        .as_str()
        .is_some());
}

#[test]
fn init_and_errors_update_the_chat() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]","permissionMode":"default"}));
    assert_eq!(t.meta().permission_mode.as_deref(), Some("default"));
    t.set_busy();
    t.apply(
        &json!({"type":"result","subtype":"error_during_execution","is_error":true,"uuid":"r1"}),
    );
    assert!(!t.meta().busy);
    assert!(matches!(&t.items()[0], TranscriptItem::Notice { .. }));
}

#[test]
fn messages_keep_the_1m_window_init_reported() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]"}));
    t.apply(&stream(
        json!({"type":"message_start","message":{"id":"m1","model":"claude-opus-5-5"}}),
    ));
    t.apply(&assistant("a1", "m1", json!({"type":"text","text":"hi"})));
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5[1m]"));
    t.apply(&json!({"type":"assistant","uuid":"a2","message":{"id":"m2","model":"claude-sonnet-5-5","content":[]}}));
    assert_eq!(t.meta().model.as_deref(), Some("claude-sonnet-5-5"));
    t.apply(&stream(
        json!({"type":"message_start","message":{"id":"m3","model":"<synthetic>"}}),
    ));
    assert_eq!(t.meta().model.as_deref(), Some("claude-sonnet-5-5"));
}

#[test]
fn picking_a_1m_model_keeps_the_1m_window() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"control_response","response":{"subtype":"success","request_id":"i",
        "response":{"models":[
            {"value":"opus[1m]","resolvedModel":"claude-opus-5-5","displayName":"Opus 5.5 (1M)"},
            {"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus 5.5"}]}}}),
    );
    t.set_model_choice("opus[1m]");
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5[1m]"));
    t.set_model_choice("opus");
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn the_context_window_comes_from_the_sessions_model_usage() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]"}));
    assert_eq!(t.meta().context_window, None);
    t.apply(
        &json!({"type":"result","subtype":"success","is_error":false,"modelUsage":{
        "claude-haiku-4-5":{"contextWindow":200000},
        "claude-opus-5-5[1m]":{"contextWindow":1000000}}}),
    );
    assert_eq!(t.meta().context_window, Some(1_000_000));
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]"}));
    assert_eq!(t.meta().context_window, Some(1_000_000));
    t.apply(&json!({"type":"assistant","uuid":"a1","message":{"id":"m1","model":"claude-sonnet-5-5","content":[]}}));
    assert_eq!(t.meta().context_window, None);
}

#[test]
fn the_plugins_one_model_sets_the_window_without_the_1m_suffix() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]"}));
    t.apply(
        &json!({"type":"result","subtype":"success","is_error":false,
        "modelUsage":{"claude-opus-5-5":{"contextWindow":1000000}}}),
    );
    assert_eq!(t.meta().context_window, Some(1_000_000));
}

#[test]
fn the_plugins_init_sets_the_window_before_any_result() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]"}));
    t.apply(&json!({"type":"system","subtype":"init","contextWindow":1000000}));
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5[1m]"));
    assert_eq!(t.meta().context_window, Some(1_000_000));
    // The first request's init and message name the bare id.
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5","effort":"high"}));
    t.apply(&json!({"type":"assistant","uuid":"a1","message":{"id":"m1","model":"claude-opus-5-5","content":[]}}));
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5[1m]"));
    assert_eq!(t.meta().context_window, Some(1_000_000));
}

#[test]
fn an_init_gaining_1m_or_another_model_forgets_the_window() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"system","subtype":"init","model":"claude-opus-5-5","contextWindow":200000}),
    );
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5[1m]"}));
    assert_eq!(t.meta().context_window, None);
    t.apply(&json!({"type":"system","subtype":"init","contextWindow":1000000}));
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-sonnet-5-5"}));
    assert_eq!(t.meta().context_window, None);
}

#[test]
fn picking_a_model_forgets_the_old_window() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"control_response","response":{"subtype":"success","request_id":"i",
        "response":{"models":[{"value":"sonnet","resolvedModel":"claude-sonnet-5-5"}]}}}),
    );
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5"}));
    t.apply(
        &json!({"type":"result","subtype":"success","is_error":false,
        "modelUsage":{"claude-opus-5-5":{"contextWindow":200000}}}),
    );
    t.set_model_choice("sonnet");
    assert_eq!(t.meta().context_window, None);
}

#[test]
fn subagent_events_are_left_to_their_task_card() {
    let mut t = Transcript::default();
    let mut line = assistant("a", "m", json!({"type":"text","text":"inside a subagent"}));
    line["parent_tool_use_id"] = json!("toolu_task");
    t.apply(&line);
    t.apply(&json!({"type":"user","uuid":"side","isSidechain":true,"message":{"content":"subagent prompt"}}));
    assert!(t.items().is_empty());
}

#[test]
fn skill_body_becomes_its_cards_output() {
    let skill_output = |t: &Transcript, i: usize| match &t.items()[i] {
        TranscriptItem::Tool { output, .. } => output.clone(),
        _ => None,
    };
    let body = "Base directory for this skill: /s\n\n# Help";
    let mut t = Transcript::default();
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_s","name":"Skill","input":{"skill":"help"}}),
    ));
    t.apply(&user(
        "r",
        json!([{"type":"tool_result","tool_use_id":"toolu_s","content":"Launching skill: help"}]),
    ));
    // stream-json: no sourceToolUseID.
    let applied = t.apply(&json!({"type":"user","isSynthetic":true,
        "message":{"role":"user","content":[{"type":"text","text":body}]}}));
    assert_eq!(applied.items, vec![0]);
    assert_eq!(skill_output(&t, 0).as_deref(), Some(body));

    // JSONL names the call; a reminder or a slash-command skill stays hidden.
    let mut t = Transcript::default();
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_s","name":"Skill","input":{"skill":"help"}}),
    ));
    t.apply(&assistant(
        "b",
        "m",
        json!({"type":"tool_use","id":"toolu_b","name":"Bash","input":{"command":"ls"}}),
    ));
    t.apply(
        &json!({"type":"user","uuid":"m1","isMeta":true,"sourceToolUseID":"toolu_s",
        "message":{"content":[{"type":"text","text":body}]}}),
    );
    t.apply(&json!({"type":"user","uuid":"m2","isSynthetic":true,
        "message":{"content":[{"type":"text","text":"Base directory for this skill: /other"}]}}));
    t.apply(&json!({"type":"user","uuid":"m3","isSynthetic":true,
        "message":{"content":"<system-reminder>x</system-reminder>"}}));
    assert_eq!(skill_output(&t, 0).as_deref(), Some(body));
    assert_eq!(skill_output(&t, 1), None);
    assert!(user_texts(&t).is_empty());
}

#[test]
fn an_unflagged_skill_body_still_goes_to_its_card() {
    let body = "Base directory for this skill: /s\n\n## `$state`";
    let mut t = Transcript::default();
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_s","name":"Skill","input":{"skill":"svelte"}}),
    ));
    t.apply(&user(
        "r",
        json!([{"type":"tool_result","tool_use_id":"toolu_s","content":"Launching skill: svelte"}]),
    ));
    t.apply(&json!({"type":"user","uuid":"b",
        "message":{"role":"user","content":[{"type":"text","text":body}]}}));
    assert!(user_texts(&t).is_empty(), "{:?}", user_texts(&t));
    assert!(
        matches!(&t.items()[0], TranscriptItem::Tool { output, .. } if output.as_deref() == Some(body))
    );
}

#[test]
fn stream_skill_bodies_go_to_launched_calls_in_order() {
    let skill_output = |t: &Transcript, i: usize| match &t.items()[i] {
        TranscriptItem::Tool { output, .. } => output.clone(),
        _ => None,
    };
    let launch = |t: &mut Transcript, id: &str, name: &str| {
        t.apply(&assistant(
            id,
            "m",
            json!({"type":"tool_use","id":id,"name":name,"input":{}}),
        ));
    };
    let result = |id: &str, text: &str| {
        user(
            &format!("r{id}"),
            json!([{"type":"tool_result","tool_use_id":id,"content":text}]),
        )
    };
    let body = |text: &str| {
        json!({"type":"user","isSynthetic":true,
            "message":{"content":[{"type":"text","text":format!("Base directory for this skill: {text}")}]}})
    };
    let mut t = Transcript::default();
    launch(&mut t, "a", "Skill");
    launch(&mut t, "b", "Skill");
    launch(&mut t, "c", "Bash");
    t.apply(&result("a", "Launching skill: a"));
    t.apply(&result("b", "Launching skill: b"));
    t.apply(&result("c", "ok"));
    t.apply(&body("/a"));
    t.apply(&body("/b"));
    assert_eq!(
        skill_output(&t, 0).as_deref(),
        Some("Base directory for this skill: /a")
    );
    assert_eq!(
        skill_output(&t, 1).as_deref(),
        Some("Base directory for this skill: /b")
    );
    assert_eq!(skill_output(&t, 2).as_deref(), Some("ok"));

    // A later `/release` loads a skill with no card: it touches no earlier one.
    t.apply(&user("cmd", json!("<command-name>/release</command-name>")));
    t.apply(&body("/release"));
    assert_eq!(
        skill_output(&t, 1).as_deref(),
        Some("Base directory for this skill: /b")
    );
}

#[test]
fn injected_messages_still_settle_tools_and_interrupts() {
    let mut t = Transcript::default();
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_x","name":"Bash","input":{}}),
    ));
    t.apply(
        &json!({"type":"user","uuid":"s","isSynthetic":true,"message":{"content":[
        {"type":"tool_result","tool_use_id":"toolu_x","content":"cancelled","is_error":true},
        {"type":"text","text":"[Request interrupted by user]"}]}}),
    );
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Tool {
            status: ToolStatus::Error,
            ..
        }
    ));
    assert!(matches!(&t.items()[1], TranscriptItem::Notice { text, .. } if text == "Interrupted"));
}

#[test]
fn hides_bookkeeping_and_shows_slash_commands_and_html() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"user","uuid":"m","isMeta":true,"message":{"content":"Base directory…"}}),
    );
    t.apply(&user(
        "s",
        json!("<local-command-stdout>done</local-command-stdout>"),
    ));
    t.apply(&user(
        "sc",
        json!("<command-name>/compact</command-name>\n<command-args>keep tests</command-args>"),
    ));
    t.apply(&assistant(
        "th",
        "m",
        json!({"type":"thinking","thinking":""}),
    ));
    t.apply(&user("ok", json!("ok")));
    t.apply(&user("html", json!("<Button> renders twice, why?")));
    t.apply(&user("custom", json!("<my-element> loses its slot")));
    assert_eq!(
        user_texts(&t),
        vec![
            "/compact keep tests",
            "ok",
            "<Button> renders twice, why?",
            "<my-element> loses its slot"
        ]
    );
}

#[test]
fn slash_commands_do_not_start_a_turn() {
    let mut t = Transcript::default();
    t.apply(&user("c", json!("<command-name>/cost</command-name>")));
    assert!(!t.meta().busy);
    t.apply(&user("p", json!("fix it")));
    assert!(t.meta().busy);
}

#[test]
fn mid_turn_prompts_are_shown() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"attachment","uuid":"q1","attachment":{
        "type":"queued_command","prompt":"also check windows ","commandMode":"prompt","timestamp":"t"}}));
    t.apply(&json!({"type":"attachment","uuid":"q2","attachment":{"type":"file","prompt":"x"}}));
    assert_eq!(user_texts(&t), vec!["also check windows"]);
}

#[test]
fn mid_turn_prompts_with_images_are_shown() {
    let mut t = Transcript::default();
    let image = json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw=="}});
    t.apply(&json!({"type":"attachment","uuid":"q1","attachment":{
        "type":"queued_command","commandMode":"prompt","timestamp":"t",
        "prompt":[image, {"type":"text","text":"stuck like this "}]}}));
    t.apply(&json!({"type":"attachment","uuid":"q2","attachment":{
        "type":"queued_command","commandMode":"prompt","timestamp":"t","prompt":[image]}}));
    assert!(
        matches!(&t.items()[0], TranscriptItem::User { text, images: 1, .. } if text == "stuck like this")
    );
    assert!(
        matches!(&t.items()[1], TranscriptItem::User { text, images: 1, .. } if text.is_empty())
    );
}

#[test]
fn interrupt_ends_the_turn() {
    let mut t = Transcript::default();
    t.apply(&user("u1", json!("go")));
    t.apply(&user(
        "u2",
        json!([{"type":"text","text":"[Request interrupted by user]"}]),
    ));
    assert!(!t.meta().busy);
    assert!(matches!(&t.items()[1], TranscriptItem::Notice { .. }));
}

#[test]
fn long_tool_input_is_clipped() {
    let mut t = Transcript::default();
    let body = "x".repeat(MAX_TEXT_BYTES * 2);
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_w","name":"Write","input":{"content":body}}),
    ));
    let TranscriptItem::Tool { input, .. } = &t.items()[0] else {
        panic!();
    };
    assert!(input["content"].as_str().unwrap().len() <= MAX_TEXT_BYTES + 3);
}

#[test]
fn load_reads_history_and_never_reports_a_turn_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("-repo");
    fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{SID}.jsonl"));
    fs::write(
        &path,
        format!(
            "{}\nnot json\n{}\n",
            user("u1", json!("hello there")),
            assistant("a1", "m1", json!({"type":"text","text":"hi"}))
        ),
    )
    .unwrap();
    assert_eq!(find_transcript(dir.path(), SID), Some(path.clone()));
    let t = Transcript::load(&path);
    assert_eq!(t.items().len(), 2);
    assert!(!t.meta().busy);
    assert!(Transcript::load(&dir.path().join("missing.jsonl"))
        .items()
        .is_empty());
}

#[test]
fn finds_and_loads_a_subagent_transcript_beside_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("-repo");
    let session = project.join(format!("{SID}.jsonl"));
    let subagents = project.join(SID).join("subagents");
    fs::create_dir_all(&subagents).unwrap();
    fs::write(&session, "").unwrap();
    let side = |mut row: Value| {
        row["isSidechain"] = json!(true);
        row
    };
    let agent = subagents.join("agent-a6ee299a623b37fc4.jsonl");
    fs::write(
        &agent,
        format!(
            "{}\n{}\n",
            side(user("u1", json!("Find the bug"))),
            side(assistant(
                "a1",
                "m1",
                json!({"type":"text","text":"Found it"})
            ))
        ),
    )
    .unwrap();
    fs::write(project.join("secret.jsonl"), "").unwrap();

    assert_eq!(
        find_subagent_transcript(&session, "a6ee299a623b37fc4"),
        Some(agent.clone())
    );
    assert_eq!(find_subagent_transcript(&session, "missing"), None);
    for bad in ["", "../../secret", "a/b", "..", "a.b", "a\\b"] {
        assert_eq!(find_subagent_transcript(&session, bad), None, "{bad:?}");
    }

    let t = Transcript::load_subagent(&agent);
    assert_eq!(user_texts(&t), ["Find the bug"]);
    assert_eq!(t.items().len(), 2);
    assert!(
        Transcript::load(&agent).items().is_empty(),
        "a session's own history still skips sidechain rows"
    );
}

#[test]
fn uuid_check_rejects_paths() {
    assert!(is_uuid(SID));
    assert!(!is_uuid("../../etc/passwd"));
    assert!(!is_uuid("7b3c54f4-ba22-4654-9af9-037d1cd8e55"));
    assert!(!is_uuid("7b3c54f4/ba22-4654-9af9-037d1cd8e555"));
}

#[test]
fn unknown_kinds_are_reported_once() {
    let mut t = Transcript::default();
    let line = json!({"type":"system","subtype":"brand_new_thing"});
    assert_eq!(
        t.apply(&line).unknown_kind.as_deref(),
        Some("system:brand_new_thing")
    );
    assert_eq!(t.apply(&line).unknown_kind, None);
}

#[test]
fn a_withdrawn_approval_expires_and_cannot_be_answered() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{}}}));
    let a = t.apply(&json!({"type":"control_cancel_request","request_id":"r"}));
    assert_eq!(a.items, vec![0]);
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Approval { expired: true, .. }
    ));
    assert!(t
        .resolve_approval("r", ApprovalDecision::Allow, None)
        .is_none());
}

fn ask(t: &mut Transcript, request_id: &str, tool: &str, tool_use_id: &str) {
    t.apply(&json!({"type":"control_request","request_id":request_id,"request":{"subtype":"can_use_tool",
        "tool_name":tool,"tool_use_id":tool_use_id,"input":{"questions":[{"question":"Which?"}]}}}));
    t.apply(&json!({"type":"control_cancel_request","request_id":request_id,"workbench_in_terminal":true}));
}

fn approval_state(t: &Transcript) -> (bool, bool, Option<ApprovalDecision>, Option<Value>) {
    match &t.items()[0] {
        TranscriptItem::Approval {
            in_terminal,
            expired,
            decision,
            answers,
            ..
        } => (*in_terminal, *expired, *decision, answers.clone()),
        other => panic!("not an approval: {other:?}"),
    }
}

#[test]
fn a_question_the_terminal_took_over_waits_there_and_shows_its_answers() {
    let mut t = Transcript::default();
    ask(&mut t, "r", "AskUserQuestion", "toolu_q");
    assert_eq!(approval_state(&t), (true, false, None, None));
    assert!(t.waiting_on().is_none(), "the chat can't answer it");
    assert!(t
        .resolve_approval("r", ApprovalDecision::Allow, None)
        .is_none());
    let result = json!({"type":"user","uuid":"res","message":{"role":"user","content":[
        {"type":"tool_result","tool_use_id":"toolu_q","content":"User answered"}]},
        "tool_use_result":{"questions":[{"question":"Which?"}],"answers":{"Which?":"B"}}});
    assert_eq!(t.apply(&result).items, vec![0]);
    assert_eq!(
        approval_state(&t),
        (
            true,
            false,
            Some(ApprovalDecision::Allow),
            Some(json!({"Which?":"B"}))
        )
    );
}

#[test]
fn an_approval_the_terminal_took_over_settles_only_when_answered_there() {
    let mut t = Transcript::default();
    ask(&mut t, "r1", "Bash", "toolu_1");
    ask(&mut t, "r2", "AskUserQuestion", "toolu_2");
    assert!(t.pending_approval_ids().is_empty());
    assert!(matches!(
        t.items()[0].waiting_summary(),
        Some(WaitingSummary {
            in_terminal: true,
            ..
        })
    ));
    // The plugin cancels it as the approved call starts.
    t.apply(&json!({"type":"control_cancel_request","request_id":"r1"}));
    assert_eq!(
        approval_state(&t),
        (true, false, Some(ApprovalDecision::Allow), None)
    );
    // Abandoned in the terminal (Esc, Stop): the turn ends unanswered.
    t.apply(&json!({"type":"result","subtype":"success"}));
    assert!(matches!(
        &t.items()[1],
        TranscriptItem::Approval {
            in_terminal: true,
            expired: true,
            decision: None,
            ..
        }
    ));
}

#[test]
fn clear_starts_over_under_the_new_session_id() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"m","permissionMode":"plan"}));
    t.apply(&user("u", json!("hello")));
    let a = t.apply(&json!({"type":"conversation_reset","new_conversation_id":SID,"uuid":"x","session_id":"old"}));
    assert_eq!(a.new_session_id.as_deref(), Some(SID));
    assert!(t.items().is_empty());
    assert_eq!(
        t.meta().permission_mode.as_deref(),
        Some("plan"),
        "mode survives /clear"
    );
}

#[test]
fn slash_command_output_is_shown() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"local_command_output","content":"Total cost: $0.42","uuid":"o1"}));
    assert!(
        matches!(&t.items()[0], TranscriptItem::Notice { text, in_terminal: false, .. } if text.contains("$0.42"))
    );
}

#[test]
fn a_command_left_open_in_the_terminal_says_so() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"local_command_output","content":"/usage opened in the terminal",
        "workbench_in_terminal":true,"uuid":"o1"}));
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Notice {
            in_terminal: true,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(&t.items()[0]).unwrap()["inTerminal"],
        json!(true)
    );
}

#[test]
fn questions_are_answered_through_updated_input() {
    let mut t = Transcript::default();
    let question = "Which library should we use?";
    t.apply(&json!({"type":"control_request","request_id":"q","request":{
        "subtype":"can_use_tool","tool_name":"AskUserQuestion",
        "input":{"questions":[{"question":question,"header":"Library","multiSelect":false,
            "options":[{"label":"date-fns","description":"Small"},{"label":"dayjs","description":"Tiny"}]}]}}}));
    let mut answers = serde_json::Map::new();
    answers.insert(question.into(), json!("dayjs"));
    answers.insert("ignored".into(), json!(42));
    let (_, response) = t
        .resolve_approval("q", ApprovalDecision::Allow, Some(&answers))
        .unwrap();
    let input = &response["response"]["response"]["updatedInput"];
    assert_eq!(
        input["answers"],
        json!({question: "dayjs"}),
        "non-strings dropped"
    );
    assert_eq!(
        input["questions"][0]["header"], "Library",
        "the questions go back unchanged"
    );
    assert!(matches!(&t.items()[0],
        TranscriptItem::Approval { answers: Some(a), .. } if a[question] == "dayjs"));
}

#[test]
fn subagents_and_background_jobs_are_tracked() {
    let mut t = Transcript::default();
    let sys = |sub: &str, extra: Value| {
        let mut v = json!({"type":"system","subtype":sub,"uuid":"x","session_id":"s"});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    t.apply(&sys(
        "task_started",
        json!({"task_id":"a1","tool_use_id":"toolu_task",
        "description":"Find flaky tests","subagent_type":"Explore","task_type":"local_agent"}),
    ));
    let a = t.apply(&sys(
        "task_progress",
        json!({"task_id":"a1","description":"Find flaky tests",
        "usage":{"total_tokens":1800,"tool_uses":7,"duration_ms":12000},"last_tool_name":"Grep"}),
    ));
    assert!(a.meta, "progress reaches clients through meta");
    t.apply(&sys(
        "task_started",
        json!({"task_id":"b1","task_type":"local_bash","description":"bun run dev",
        "is_backgrounded":true}),
    ));
    t.apply(&sys(
        "task_notification",
        json!({"task_id":"a1","status":"completed",
        "output_file":"/tmp/o","summary":"Two tests depend on wall-clock time."}),
    ));
    t.apply(&sys(
        "task_updated",
        json!({"task_id":"b1","patch":{"status":"failed","error":"exit 1"}}),
    ));

    let tasks = &t.meta().tasks;
    assert_eq!(tasks.len(), 2);
    let agent = &tasks[0];
    assert_eq!(
        (agent.kind.as_str(), agent.status.as_str()),
        ("agent", "completed")
    );
    assert_eq!(agent.subagent_type.as_deref(), Some("Explore"));
    assert_eq!((agent.tool_uses, agent.tokens), (7, 1800));
    assert_eq!(agent.last_tool.as_deref(), Some("Grep"));
    assert_eq!(
        agent.description, "Find flaky tests",
        "progress keeps the task's name"
    );
    assert_eq!(agent.activity, None, "a finished task has no current step");
    assert!(agent.summary.as_deref().unwrap().contains("wall-clock"));
    let shell = &tasks[1];
    assert_eq!(
        (shell.kind.as_str(), shell.status.as_str()),
        ("local_bash", "failed")
    );
    assert!(shell.background);
    assert_eq!(shell.description, "bun run dev");
}

#[test]
fn usage_limits_reach_the_chat() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"rate_limit_event","rate_limit_info":{"status":"rejected",
        "resetsAt":1790857800,"rateLimitType":"five_hour","utilization":1.0}}),
    );
    let limit = t.meta().rate_limit.clone().unwrap();
    assert_eq!(limit.status, "rejected");
    assert_eq!(limit.resets_at, Some(1790857800));
    assert_eq!(limit.kind.as_deref(), Some("five_hour"));
}

#[test]
fn long_tool_output_keeps_a_preview_and_the_whole_text() {
    let mut t = Transcript::default();
    t.apply(&assistant(
        "a",
        "m",
        json!({"type":"tool_use","id":"toolu_l","name":"Bash","input":{}}),
    ));
    let long = "line\n".repeat(3000);
    t.apply(&json!({"type":"user","uuid":"u","message":{"content":[
        {"type":"tool_result","tool_use_id":"toolu_l","content":long}]}}));
    let TranscriptItem::Tool {
        output: Some(preview),
        full_output_bytes: Some(bytes),
        ..
    } = &t.items()[0]
    else {
        panic!("expected a preview");
    };
    assert!(preview.len() <= MAX_TEXT_BYTES + 3);
    assert_eq!(*bytes, long.len());
    assert_eq!(t.full_output("toolu_l"), Some(long.as_str()));
}

#[test]
fn attached_documents_are_named_on_the_message() {
    let mut t = Transcript::default();
    let pdf = json!({"type":"document","title":"report.pdf",
        "source":{"type":"base64","media_type":"application/pdf","data":"JVBERg=="}});
    let text = json!({"type":"document","title":"main.rs",
        "source":{"type":"text","media_type":"text/plain","data":"fn main() {}"}});
    t.apply(&user(
        "u1",
        json!([pdf.clone(), text, {"type":"text","text":"summarise"}]),
    ));
    t.apply(&user("u2", json!([pdf.clone()])));
    t.apply(&json!({"type":"attachment","uuid":"q1","attachment":{
        "type":"queued_command","commandMode":"prompt","timestamp":"t","prompt":[pdf]}}));
    let files = |i: usize| match &t.items()[i] {
        TranscriptItem::User { text, files, .. } => (text.clone(), files.clone()),
        other => panic!("expected a user item, got {other:?}"),
    };
    assert_eq!(
        files(0),
        (
            "summarise".into(),
            vec!["report.pdf".into(), "main.rs".into()]
        )
    );
    assert_eq!(files(1), (String::new(), vec!["report.pdf".into()]));
    assert_eq!(files(2), (String::new(), vec!["report.pdf".into()]));
    let json = serde_json::to_value(&t.items()[1]).unwrap();
    assert_eq!(json["files"], json!(["report.pdf"]));
    assert!(json.get("images").is_none());
}

#[test]
fn pasted_images_are_counted_on_the_message() {
    let mut t = Transcript::default();
    let image =
        json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}});
    t.apply(&user(
        "u1",
        json!([image.clone(), {"type":"text","text":"what's wrong here?"}]),
    ));
    t.apply(&user("u2", json!([image])));
    assert!(matches!(&t.items()[0],
        TranscriptItem::User { text, images: 1, .. } if text == "what's wrong here?"));
    assert!(
        matches!(&t.items()[1], TranscriptItem::User { text, images: 1, .. } if text.is_empty()),
        "an image on its own is still a message"
    );
}

#[test]
fn the_initialize_reply_lists_models_to_pick_from() {
    let mut t = Transcript::default();
    let a = t.apply(&json!({"type":"control_response","response":{"subtype":"success","request_id":"i",
        "response":{"models":[
            {"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus 5.5",
             "description":"For complex work","supportsEffort":true,"supportedEffortLevels":["low","high","max"]},
            {"value":"haiku","resolvedModel":"claude-haiku-4-5","displayName":"Haiku 4.5","description":"Fastest"}]}}}));
    assert!(a.meta);
    let models = &t.meta().models;
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].effort_levels, vec!["low", "high", "max"]);
    assert!(models[1].effort_levels.is_empty());

    t.set_model_choice("haiku");
    assert_eq!(t.meta().model_choice.as_deref(), Some("haiku"));
    assert_eq!(t.meta().model.as_deref(), Some("claude-haiku-4-5"));
    t.set_effort("high");
    assert_eq!(t.meta().effort.as_deref(), Some("high"));
}

#[test]
fn pinned_models_outlast_a_plugin_list_and_its_choice_and_effort_show() {
    let real = |value: &str, resolved: &str| ModelOption {
        value: value.into(),
        display_name: value.into(),
        description: String::new(),
        resolved_model: Some(resolved.into()),
        default_effort: None,
        input_modalities: Vec::new(),
        service_tiers: Vec::new(),
        effort_levels: vec!["low".into(), "high".into()],
    };
    let mut t = Transcript::default();
    t.pin_models(vec![
        real("default", "claude-opus-5-5"),
        real("sonnet", "claude-sonnet-5-5"),
    ]);
    // A terminal's plugin: guessed list, plus the pick `/config` holds.
    let a = t.apply(
        &json!({"type":"control_response","response":{"subtype":"success","request_id":"i",
        "response":{"models":[{"value":"sonnet","displayName":"Sonnet"}],"modelChoice":"sonnet"}}}),
    );
    assert!(a.meta);
    assert_eq!(t.meta().models.len(), 2);
    assert_eq!(t.meta().model_choice.as_deref(), Some("sonnet"));
    assert_eq!(t.meta().model.as_deref(), Some("claude-sonnet-5-5"));

    t.apply(&json!({"type":"permission-mode","permissionMode":"plan"}));
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-sonnet-5-5","effort":"high"}));
    assert_eq!(t.meta().effort.as_deref(), Some("high"));
    assert_eq!(t.meta().permission_mode.as_deref(), Some("plan"));
}

#[test]
fn a_prompt_put_into_a_running_turn_shows_as_typed() {
    let mut t = Transcript::default();
    let framed = format!(
        "{QUEUED_PROMPT_PREFIX}look at @/tmp/a.png\n\nAttached files (read each with the Read tool):\n- /tmp/a.png\n\nIMPORTANT: do it"
    );
    t.apply(&json!({"type":"user","uuid":"q","isMeta":true,"message":{"content":framed}}));
    assert!(
        matches!(t.items(), [TranscriptItem::User { text, .. }] if text == "look at @/tmp/a.png"),
        "{:?}",
        t.items()
    );
}

#[test]
fn a_task_can_name_the_agent_whose_file_holds_its_output() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"task_started","task_id":"toolu_1","description":"Track"}));
    t.apply(&json!({"type":"system","subtype":"task_updated","task_id":"toolu_1","output_id":"a6ee299a623b37fc4"}));
    assert_eq!(
        t.meta().tasks[0].output_id.as_deref(),
        Some("a6ee299a623b37fc4")
    );
    assert_eq!(t.meta().tasks[0].status, "running");
}

#[test]
fn slash_commands_come_from_initialize_and_updates() {
    let mut t = Transcript::default();
    let a = t.apply(&json!({"type":"control_response","response":{"subtype":"success","request_id":"i",
        "response":{"commands":[{"name":"compact","description":"Free up context","argumentHint":"<focus>"},
            {"name":"clear","description":"Start over","argumentHint":""}]}}}));
    assert!(a.commands);
    assert_eq!(t.commands()[0].argument_hint.as_deref(), Some("<focus>"));
    assert_eq!(t.commands()[1].argument_hint, None);
    t.apply(&json!({"type":"conversation_reset","new_conversation_id":SID}));
    assert_eq!(t.commands().len(), 2, "/clear keeps the command list");
    let a = t.apply(&json!({"type":"workbench_commands",
        "commands":[{"name":"review","description":"Review a diff"}]}));
    assert!(a.commands);
    assert_eq!(a.unknown_kind, None);
    assert_eq!(
        t.commands()[0].name,
        "review",
        "the plugin's refresh replaces the list"
    );
}

#[test]
fn a_refused_model_switch_puts_the_old_model_back() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"control_response","response":{"subtype":"success","request_id":"i",
        "response":{"models":[{"value":"sonnet","resolvedModel":"claude-sonnet-5-5"}],"modelChoice":"default"}}}),
    );
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5"}));
    t.request_model("m1", "sonnet");
    assert_eq!(t.meta().model.as_deref(), Some("claude-sonnet-5-5"));
    let a = t.apply(
        &json!({"type":"control_response","response":{"subtype":"error","request_id":"m1",
        "error":"Model: a managed setting fixes it"}}),
    );
    assert!(a.meta);
    assert_eq!(t.meta().model_choice.as_deref(), Some("default"));
    assert_eq!(t.meta().model.as_deref(), Some("claude-opus-5-5"));
    assert!(matches!(t.items(), [TranscriptItem::Notice { text, .. }] if text.contains("managed")));

    t.request_model("m2", "sonnet");
    t.apply(&json!({"type":"control_response","response":{"subtype":"success","request_id":"m2","response":{}}}));
    assert_eq!(t.meta().model_choice.as_deref(), Some("sonnet"));
}

#[test]
fn each_refused_model_switch_undoes_only_its_own_pick() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"system","subtype":"init","model":"claude-opus-5-5","modelChoice":"opus"}),
    );
    t.request_model("a", "sonnet");
    t.request_model("b", "haiku");
    let refuse = |id: &str| json!({"type":"control_response","response":{"subtype":"error","request_id":id,"error":"no"}});
    t.apply(&refuse("a"));
    assert_eq!(
        t.meta().model_choice.as_deref(),
        Some("haiku"),
        "the later pick stays"
    );
    t.apply(&refuse("b"));
    assert_eq!(
        t.meta().model_choice.as_deref(),
        Some("opus"),
        "not the refused sonnet"
    );
}

#[test]
fn a_model_switch_in_the_terminal_names_its_pick() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-sonnet-5-5","modelChoice":"sonnet"}));
    assert_eq!(t.meta().model_choice.as_deref(), Some("sonnet"));
    assert_eq!(t.meta().model.as_deref(), Some("claude-sonnet-5-5"));
}

#[test]
fn a_terminal_elicitation_shows_read_only_until_answered() {
    let mut t = Transcript::default();
    let a = t.apply(&json!({"type":"workbench_terminal_elicitation","id":"e1",
        "mcp_server_name":"deploy","message":"Which environment?","mode":"form",
        "requested_schema":{"type":"object","properties":{"env":{"type":"string"}}}}));
    assert_eq!(a.unknown_kind, None);
    assert!(matches!(&t.items()[0],
        TranscriptItem::Elicitation { in_terminal: true, action: None, server, .. } if server == "deploy"));
    assert!(
        t.waiting_on().is_none(),
        "the terminal asks it, not the chat"
    );
    let json = serde_json::to_value(&t.items()[0]).unwrap();
    assert_eq!(json["inTerminal"], true);

    t.apply(&json!({"type":"workbench_terminal_elicitation_result","id":"e1","action":"accept"}));
    assert!(matches!(
        &t.items()[0],
        TranscriptItem::Elicitation {
            action: Some(ElicitationAction::Accept),
            ..
        }
    ));

    t.apply(&json!({"type":"workbench_terminal_elicitation","id":"e2","mcp_server_name":"x","message":"?"}));
    t.apply(&json!({"type":"result","subtype":"success","is_error":false}));
    assert!(
        matches!(
            &t.items()[1],
            TranscriptItem::Elicitation { expired: true, .. }
        ),
        "unanswered when the turn ended"
    );
}

#[test]
fn resume_shows_the_resumed_conversations_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{SID}.jsonl"));
    std::fs::write(&path, user("h1", json!("from before")).to_string()).unwrap();
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"m","permissionMode":"plan"}));
    t.apply(&user("u", json!("hello")));
    t.apply(&json!({"type":"conversation_reset","new_conversation_id":SID}));
    t.resume_history(&path);
    assert!(matches!(t.items(), [TranscriptItem::User { text, .. }] if text == "from before"));
    assert_eq!(t.meta().permission_mode.as_deref(), Some("plan"));
    assert!(!t.meta().busy);
}

#[test]
fn plugin_framing_stays_out_of_the_chat() {
    let mut t = Transcript::default();
    let idle =
        "look at @/tmp/a.png\n\nAttached files (read each with the Read tool):\n- /tmp/a.png";
    t.apply(&user("p1", json!(idle)));
    t.apply(&json!({"type":"result","subtype":"success","is_error":false}));
    t.apply(&user("p2", json!(QUEUED_NUDGE)));
    assert!(
        matches!(t.items(), [TranscriptItem::User { text, .. }] if text == "look at @/tmp/a.png")
    );
    assert!(t.meta().busy, "the nudge still starts a turn");
}

#[test]
fn a_pick_resolves_through_the_model_list() {
    let mut t = Transcript::default();
    t.apply(
        &json!({"type":"control_response","response":{"subtype":"success","request_id":"i",
        "response":{"models":[{"value":"opus[1m]","resolvedModel":"claude-opus-5-5"},
            {"value":"haiku"}]}}}),
    );
    assert_eq!(
        t.resolve_model("opus[1m]").as_deref(),
        Some("claude-opus-5-5[1m]")
    );
    assert_eq!(
        t.resolve_model("claude-fable-5-1").as_deref(),
        Some("claude-fable-5-1")
    );
    assert_eq!(t.resolve_model("haiku"), None, "a stand-in entry has no id");
    assert_eq!(t.resolve_model("sonnet"), None);
}

#[test]
fn an_init_without_effort_clears_it_only_when_it_says_so() {
    let mut t = Transcript::default();
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5","effort":"high"}));
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-opus-5-5"}));
    assert_eq!(t.meta().effort.as_deref(), Some("high"));
    t.apply(&json!({"type":"system","subtype":"init","model":"claude-haiku-4-5","effort":null}));
    assert_eq!(t.meta().effort, None);
}

#[test]
fn a_command_echo_shows_as_typed_without_starting_a_turn() {
    // The plugin forwards a command's own transcript row (`/goal`, `/rename`).
    let mut t = Transcript::default();
    t.apply(&user(
        "c1",
        json!("<command-name>/goal</command-name>\n            <command-message>goal</command-message>\n            <command-args>tests pass</command-args>"),
    ));
    assert_eq!(user_texts(&t), vec!["/goal tests pass"]);
    assert!(!t.meta().busy);
}

#[test]
fn compacting_shows_from_its_status_until_the_boundary_or_turn_end() {
    let mut t = Transcript::default();
    let status = |s: Value| json!({"type":"system","subtype":"status","status":s});
    t.apply(&status(json!("compacting")));
    let since = t.meta().compacting_since.expect("compacting");
    t.apply(&status(json!("compacting")));
    assert_eq!(
        t.meta().compacting_since,
        Some(since),
        "a repeat keeps the start"
    );
    t.apply(&json!({"type":"system","subtype":"compact_boundary","uuid":"c"}));
    assert_eq!(t.meta().compacting_since, None);

    t.apply(&status(json!("compacting")));
    t.apply(&json!({"type":"result","subtype":"success","is_error":false}));
    assert_eq!(
        t.meta().compacting_since,
        None,
        "a turn that ended mid-compact"
    );

    t.apply(&status(json!("compacting")));
    t.apply(&status(Value::Null));
    assert_eq!(t.meta().compacting_since, None, "skipped");
}
