use super::*;

/// A legacy v1 `workspaces.json`, as desktops before the server-owned model
/// wrote it (with `serverTerminalIds`), real paths replaced.
const V1: &str = r#"{
  "workspaces": [
    {
      "id": "ws-main",
      "projectPath": "/home/dev/app",
      "projectName": "app",
      "terminalTabs": [
        {
          "id": "tab-shell",
          "label": "Terminal 1",
          "split": "horizontal",
          "panes": [{ "id": "pane-shell", "startupCommand": "bun run dev" }]
        },
        {
          "id": "tab-claude",
          "label": "Fix login",
          "split": "vertical",
          "type": "claude",
          "panes": [
            {
              "id": "pane-claude",
              "type": "claude",
              "claudeSessionId": "8d0c3a52-1111-4a7e-9a55-0f6a3e2b1c01",
              "view": "chat",
              "liveTerminal": true,
              "claudeAccountId": "work",
              "claudePrompt": "Review the diff"
            },
            {
              "id": "pane-claude-2",
              "type": "claude",
              "claudeSessionId": "8d0c3a52-2222-4a7e-9a55-0f6a3e2b1c02"
            }
          ]
        },
        {
          "id": "tab-legacy-claude",
          "label": "Claude 2",
          "split": "horizontal",
          "type": "claude",
          "panes": [{ "id": "pane-legacy", "type": "claude", "startupCommand": "claude" }]
        },
        {
          "id": "tab-codex",
          "label": "Codex 1",
          "split": "horizontal",
          "type": "codex",
          "panes": [
            {
              "id": "pane-codex",
              "type": "codex",
              "claudeSessionId": "",
              "startupCommand": "codex --no-daemon -c tui.alternate_screen=never -c approval_policy=never 'it'\"'\"'s broken'"
            }
          ]
        },
        {
          "id": "tab-codex-chat",
          "label": "Codex 2",
          "split": "horizontal",
          "type": "codex",
          "panes": [
            {
              "id": "pane-codex-chat",
              "type": "codex",
              "claudeSessionId": "0199aa00-3333-7000-8000-000000000003",
              "startupCommand": "codex -c tui.alternate_screen=never resume 0199aa00-3333-7000-8000-000000000003",
              "view": "chat"
            }
          ]
        },
        { "id": "tab-empty", "label": "Terminal 6", "split": "horizontal", "panes": [] }
      ],
      "activeTerminalTabId": "tab-claude",
      "splitView": { "direction": "vertical", "tabIds": ["tab-shell", "tab-claude"] },
      "branch": "main"
    },
    {
      "id": "ws-wt",
      "projectPath": "/home/dev/app",
      "projectName": "app",
      "terminalTabs": [
        {
          "id": "tab-task",
          "label": "login",
          "split": "horizontal",
          "panes": [
            {
              "id": "pane-task",
              "startupCommand": "claude auth login",
              "claudeAccountId": "work",
              "serverTerminalId": "term-9"
            }
          ]
        }
      ],
      "activeTerminalTabId": "gone",
      "splitView": { "direction": "horizontal", "tabIds": ["tab-task", "gone"] },
      "worktreePath": "/home/dev/app-feat",
      "branch": "feat",
      "renderer": "native"
    }
  ],
  "selectedId": "ws-wt",
  "serverTerminalIds": { "pane-shell": "term-1", "pane-claude": "term-2" }
}"#;

fn pane<'a>(m: &'a Model, id: &str) -> &'a Pane {
    m.pane(id).unwrap_or_else(|| panic!("pane {id} lost"))
}

#[test]
fn migrates_the_current_desktop_snapshot_without_losing_fields() {
    let file = parse(V1).unwrap();
    let m = &file.model;
    assert_eq!(m.workspaces.len(), 2);

    let main = &m.workspaces[0];
    assert_eq!(main.id, "ws-main");
    assert_eq!(main.project_path, "/home/dev/app");
    assert_eq!(main.project_name, "app");
    assert_eq!(main.worktree_path, None);
    assert_eq!(
        main.branch, None,
        "a main checkout's branch is read from git"
    );
    assert_eq!(main.renderer, Renderer::Xterm);
    assert!(!main.transient);
    assert_eq!(
        main.split_view,
        Some(SplitView {
            direction: SplitDirection::Vertical,
            tab_ids: ["tab-shell".into(), "tab-claude".into()],
        })
    );
    let tabs: Vec<_> = main
        .tabs
        .iter()
        .map(|t| (t.id.as_str(), t.label.as_str(), t.kind, t.split))
        .collect();
    assert_eq!(
        tabs,
        [
            (
                "tab-shell",
                "Terminal 1",
                PaneKind::Shell,
                SplitDirection::Horizontal
            ),
            (
                "tab-claude",
                "Fix login",
                PaneKind::Claude,
                SplitDirection::Vertical
            ),
            (
                "tab-legacy-claude",
                "Claude 2",
                PaneKind::Claude,
                SplitDirection::Horizontal
            ),
            (
                "tab-codex",
                "Codex 1",
                PaneKind::Codex,
                SplitDirection::Horizontal
            ),
            (
                "tab-codex-chat",
                "Codex 2",
                PaneKind::Codex,
                SplitDirection::Horizontal
            ),
            (
                "tab-empty",
                "Terminal 6",
                PaneKind::Shell,
                SplitDirection::Horizontal
            ),
        ]
    );

    let shell = pane(m, "pane-shell");
    assert_eq!(shell.kind, PaneKind::Shell);
    assert_eq!(shell.command.as_deref(), Some("bun run dev"));

    let claude = pane(m, "pane-claude");
    assert_eq!(
        claude.session_id.as_deref(),
        Some("8d0c3a52-1111-4a7e-9a55-0f6a3e2b1c01")
    );
    assert_eq!(claude.account_id.as_deref(), Some("work"));
    assert_eq!(claude.prompt.as_deref(), Some("Review the diff"));
    assert_eq!(claude.command, None);
    assert_eq!(claude.codex_mode, None);
    assert_eq!(
        pane(m, "pane-claude-2").session_id.as_deref(),
        Some("8d0c3a52-2222-4a7e-9a55-0f6a3e2b1c02")
    );

    // An older build's `claude` command with no id: a new session, no command.
    let legacy = pane(m, "pane-legacy");
    assert_eq!(legacy.session_id.as_ref().map(String::len), Some(36));
    assert_eq!(legacy.command, None);

    let codex = pane(m, "pane-codex");
    assert_eq!(codex.session_id, None);
    assert_eq!(codex.prompt.as_deref(), Some("it's broken"));
    assert_eq!(codex.codex_mode, Some(CodexMode::Tui));
    let codex_chat = pane(m, "pane-codex-chat");
    assert_eq!(
        codex_chat.session_id.as_deref(),
        Some("0199aa00-3333-7000-8000-000000000003")
    );
    assert_eq!(codex_chat.prompt, None);
    assert_eq!(codex_chat.codex_mode, Some(CodexMode::AppServer));

    let empty = &main.tabs[5];
    assert_eq!(empty.panes.len(), 1);
    assert_eq!(empty.panes[0].kind, PaneKind::Shell);

    let wt = &m.workspaces[1];
    assert_eq!(wt.worktree_path.as_deref(), Some("/home/dev/app-feat"));
    assert_eq!(wt.branch.as_deref(), Some("feat"));
    assert_eq!(wt.renderer, Renderer::Native);
    assert_eq!(wt.split_view, None, "a split naming a missing tab is stale");
    let task = pane(m, "pane-task");
    assert_eq!(task.command.as_deref(), Some("claude auth login"));
    assert_eq!(task.account_id.as_deref(), Some("work"));

    let local = &file.local;
    assert_eq!(local.selected_id.as_deref(), Some("ws-wt"));
    assert_eq!(
        local.active_tab_ids,
        BTreeMap::from([("ws-main".into(), "tab-claude".into())])
    );
    assert_eq!(local.chat_panes, ["pane-claude"]);
    assert_eq!(
        local.server_terminal_ids,
        BTreeMap::from([
            ("pane-claude".into(), "term-2".into()),
            ("pane-shell".into(), "term-1".into()),
            ("pane-task".into(), "term-9".into()),
        ])
    );
}

#[test]
fn reads_a_snapshot_without_server_terminal_ids() {
    let file = parse(r#"{ "workspaces": [], "selectedId": null }"#).unwrap();
    assert_eq!(file, WorkspacesFile::default());
}

#[test]
fn save_then_load_round_trips_and_leaves_the_old_file_for_older_builds() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(LEGACY_FILE);
    std::fs::write(&legacy, V1).unwrap();
    let mut file = load(dir.path()).unwrap();
    assert_eq!(
        file.local,
        parse(V1).unwrap().local,
        "migrated from the old file"
    );
    file.model.workspaces[0].transient = true;
    file.model.workspaces[0].tabs[1].panes[0].previous_ids = vec!["old".into()];
    save(dir.path(), &file).unwrap();

    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), V1);
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(FILE)).unwrap()).unwrap();
    assert_eq!(raw["version"], VERSION);
    assert_eq!(
        raw["workspaces"][0]["tabs"][1]["panes"][0]["sessionId"],
        "8d0c3a52-1111-4a7e-9a55-0f6a3e2b1c01"
    );
    assert_eq!(raw["local"]["chatPanes"][0], "pane-claude");

    assert_eq!(load(dir.path()).unwrap(), file, "the v2 file wins");
}

#[test]
fn a_missing_file_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load(dir.path()).unwrap(), WorkspacesFile::default());
}

#[test]
fn versions_are_read_explicitly() {
    let v1 = parse(r#"{ "version": 1, "workspaces": [], "selectedId": "w" }"#).unwrap();
    assert_eq!(v1.local.selected_id.as_deref(), Some("w"));
    assert!(parse(r#"{ "version": 2, "workspaces": [] }"#).is_ok());
    for bad in [
        r#"99"#, r#"3"#, r#"0"#, r#""2""#, r#"2.0"#, r#"null"#, r#"-1"#,
    ] {
        let err = parse(&format!(r#"{{ "version": {bad}, "workspaces": [] }}"#)).unwrap_err();
        assert!(err.to_string().contains("version"), "{bad}: {err}");
    }
}

#[test]
fn a_tab_mixing_kinds_is_split_by_kind() {
    let file = parse(
        r#"{ "workspaces": [{
            "id": "w", "projectPath": "/p", "projectName": "p", "activeTerminalTabId": "t",
            "terminalTabs": [{ "id": "t", "label": "Mixed", "split": "vertical", "type": "claude",
              "panes": [
                { "id": "s1" },
                { "id": "c1", "type": "claude", "claudeSessionId": "c" },
                { "id": "s2", "type": "shell" }
              ] }]
        }] }"#,
    )
    .unwrap();
    let tabs = &file.model.workspaces[0].tabs;
    assert_eq!(tabs.len(), 2);
    // A pane without a type takes its tab's: `s1` is Claude, as the tab says.
    assert_eq!(tabs[0].id, "t");
    assert_eq!(tabs[0].kind, PaneKind::Claude);
    let ids = |t: &Tab| t.panes.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&tabs[0]), ["s1", "c1"]);
    assert_eq!(tabs[1].kind, PaneKind::Shell);
    assert_eq!(ids(&tabs[1]), ["s2"]);
    assert_eq!(tabs[1].label, "Mixed");
    assert_eq!(tabs[1].split, SplitDirection::Vertical);
    for tab in tabs {
        assert!(tab.panes.iter().all(|p| p.kind == tab.kind));
    }
}

#[test]
fn codex_prompt_matches_the_desktop_extraction() {
    assert_eq!(codex_prompt("codex -c tui.alternate_screen=never"), None);
    assert_eq!(
        codex_prompt("codex -c tui.alternate_screen=never -c sandbox_mode=read-only 'fix it'"),
        Some("fix it".into())
    );
    assert_eq!(
        codex_prompt(r#"codex -c tui.alternate_screen=never "say \"hi\"""#),
        Some(r#"say "hi""#.into())
    );
    assert_eq!(codex_prompt("bash"), None);
}
