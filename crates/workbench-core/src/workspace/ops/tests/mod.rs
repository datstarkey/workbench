use super::*;

const PROJECT: &str = "/repo/app";
const WORKTREE: &str = "/repo/app-feat";

fn open(model: &mut Model, worktree: Option<&str>) -> String {
    let effects = apply(
        model,
        Command::OpenWorkspace {
            project_path: PROJECT.into(),
            project_name: "app".into(),
            worktree_path: worktree.map(Into::into),
            branch: worktree.map(|_| "feat".into()),
            renderer: Renderer::Xterm,
        },
    )
    .unwrap();
    match &effects[0] {
        Effect::Opened { workspace_id, .. } => workspace_id.clone(),
        e => panic!("expected Opened, got {e:?}"),
    }
}

fn session(kind: PaneKind) -> Command {
    Command::NewSession {
        target: Target::Workspace {
            workspace_id: String::new(),
        },
        kind,
        resume: None,
        prompt: None,
        account_id: None,
        label: None,
        command: None,
        codex_mode: None,
    }
}

fn in_ws(cmd: Command, ws: &str) -> Command {
    match cmd {
        Command::NewSession {
            kind,
            resume,
            prompt,
            account_id,
            label,
            command,
            codex_mode,
            ..
        } => Command::NewSession {
            target: Target::Workspace {
                workspace_id: ws.into(),
            },
            kind,
            resume,
            prompt,
            account_id,
            label,
            command,
            codex_mode,
        },
        c => c,
    }
}

/// Runs a NewSession and returns `(tab id, pane id, effects)`.
fn new(model: &mut Model, ws: &str, cmd: Command) -> (String, String, Vec<Effect>) {
    let effects = apply(model, in_ws(cmd, ws)).unwrap();
    let (tab, pane) = effects
        .iter()
        .find_map(|e| match e {
            Effect::Opened {
                tab_id: Some(t),
                pane_id: Some(p),
                ..
            } => Some((t.clone(), p.clone())),
            _ => None,
        })
        .expect("NewSession reports the pane");
    (tab, pane, effects)
}

fn with(cmd: Command, f: impl FnOnce(&mut Command)) -> Command {
    let mut cmd = cmd;
    f(&mut cmd);
    cmd
}

fn labels(model: &Model, ws: &str) -> Vec<String> {
    model
        .workspace(ws)
        .unwrap()
        .tabs
        .iter()
        .map(|t| t.label.clone())
        .collect()
}

fn ends(effects: &[Effect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::End { pane_id, .. } => Some(pane_id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn open_workspace_reuses_main_and_worktree_separately() {
    let mut m = Model::default();
    let main = open(&mut m, None);
    let wt = open(&mut m, Some(WORKTREE));
    assert_ne!(main, wt);
    assert_eq!(open(&mut m, None), main);
    assert_eq!(open(&mut m, Some(WORKTREE)), wt);
    // A worktree path equal to the project is the main checkout.
    assert_eq!(open(&mut m, Some(PROJECT)), main);
    assert_eq!(m.workspaces.len(), 2);
    let wt_ws = m.workspace(&wt).unwrap();
    assert_eq!(wt_ws.cwd(), WORKTREE);
    assert_eq!(wt_ws.branch.as_deref(), Some("feat"));
    // Re-opening changes nothing, so nothing is saved.
    let again = apply(
        &mut m,
        Command::OpenWorkspace {
            project_path: PROJECT.into(),
            project_name: "app".into(),
            worktree_path: None,
            branch: None,
            renderer: Renderer::Xterm,
        },
    )
    .unwrap();
    assert!(!again.contains(&Effect::Persist));
}

#[test]
fn labels_number_per_kind_and_shells_count_every_tab() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    new(&mut m, &ws, session(PaneKind::Claude));
    new(&mut m, &ws, session(PaneKind::Claude));
    new(&mut m, &ws, session(PaneKind::Codex));
    new(&mut m, &ws, session(PaneKind::Shell));
    let named = with(session(PaneKind::Claude), |c| {
        if let Command::NewSession { label, .. } = c {
            *label = Some("  Review  ".into());
        }
    });
    new(&mut m, &ws, named);
    let blank = with(session(PaneKind::Claude), |c| {
        if let Command::NewSession { label, .. } = c {
            *label = Some("   ".into());
        }
    });
    new(&mut m, &ws, blank);
    assert_eq!(
        labels(&m, &ws),
        [
            "Claude 1",
            "Claude 2",
            "Codex 1",
            "Terminal 4",
            "Review",
            "Claude 4"
        ]
    );
}

#[test]
fn new_claude_picks_its_id_up_front_and_spawns() {
    let mut m = Model::default();
    let ws = open(&mut m, Some(WORKTREE));
    let cmd = with(session(PaneKind::Claude), |c| {
        if let Command::NewSession {
            prompt, account_id, ..
        } = c
        {
            *prompt = Some("  fix the bug ".into());
            *account_id = Some("work".into());
        }
    });
    let (tab, pane, effects) = new(&mut m, &ws, cmd);
    let p = m.pane(&pane).unwrap();
    let id = p.session_id.clone().unwrap();
    assert_eq!(id.len(), 36);
    assert_eq!(p.prompt.as_deref(), Some("fix the bug"));
    assert_eq!(
        effects,
        vec![
            Effect::SpawnClaude {
                pane_id: pane.clone(),
                cwd: WORKTREE.into(),
                renderer: Renderer::Xterm,
                session_id: id,
                resume: false,
                account_id: Some("work".into()),
                prompt: Some("fix the bug".into()),
            },
            Effect::Opened {
                workspace_id: ws,
                tab_id: Some(tab),
                pane_id: Some(pane),
            },
            Effect::Persist,
        ]
    );
}

#[test]
fn new_codex_and_shell_spawn_with_their_fields() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let codex = with(session(PaneKind::Codex), |c| {
        if let Command::NewSession { prompt, .. } = c {
            *prompt = Some("review".into());
        }
    });
    let (_, pane, effects) = new(&mut m, &ws, codex);
    assert_eq!(m.pane(&pane).unwrap().codex_mode, Some(CodexMode::Tui));
    assert!(effects.contains(&Effect::SpawnCodex {
        pane_id: pane,
        cwd: PROJECT.into(),
        renderer: Renderer::Xterm,
        session_id: None,
        mode: CodexMode::Tui,
        prompt: Some("review".into()),
    }));

    let task = with(session(PaneKind::Shell), |c| {
        if let Command::NewSession {
            label,
            command,
            account_id,
            ..
        } = c
        {
            *label = Some("login".into());
            *command = Some("claude auth login".into());
            *account_id = Some("work".into());
        }
    });
    let (_, pane, effects) = new(&mut m, &ws, task);
    assert!(effects.contains(&Effect::SpawnShell {
        pane_id: pane,
        cwd: PROJECT.into(),
        renderer: Renderer::Xterm,
        command: Some("claude auth login".into()),
        account_id: Some("work".into()),
    }));
}

#[test]
fn new_session_rejects_mismatched_fields() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let bad = [
        with(session(PaneKind::Shell), |c| {
            if let Command::NewSession { prompt, .. } = c {
                *prompt = Some("x".into());
            }
        }),
        with(session(PaneKind::Claude), |c| {
            if let Command::NewSession { command, .. } = c {
                *command = Some("ls".into());
            }
        }),
        with(session(PaneKind::Claude), |c| {
            if let Command::NewSession { codex_mode, .. } = c {
                *codex_mode = Some(CodexMode::AppServer);
            }
        }),
    ];
    for cmd in bad {
        assert!(apply(&mut m, in_ws(cmd, &ws)).is_err());
    }
    assert!(apply(&mut m, in_ws(session(PaneKind::Shell), "nope")).is_err());
    assert!(m.workspace(&ws).unwrap().tabs.is_empty());
}

#[test]
fn resume_of_an_open_session_returns_its_pane() {
    let mut m = Model::default();
    let main = open(&mut m, None);
    let wt = open(&mut m, Some(WORKTREE));
    let resume = |id: &str| {
        with(session(PaneKind::Claude), |c| {
            if let Command::NewSession { resume, prompt, .. } = c {
                *resume = Some(id.into());
                *prompt = Some("ignored".into());
            }
        })
    };
    let (tab, pane, effects) = new(&mut m, &main, resume("s-1"));
    assert!(matches!(
        &effects[0],
        Effect::SpawnClaude { resume: true, session_id, prompt: None, .. } if session_id == "s-1"
    ));
    assert_eq!(m.pane(&pane).unwrap().prompt, None);

    // Resumed from another workspace: same pane, no process, nothing saved.
    let again = apply(&mut m, in_ws(resume("s-1"), &wt)).unwrap();
    assert_eq!(
        again,
        vec![Effect::Opened {
            workspace_id: main.clone(),
            tab_id: Some(tab),
            pane_id: Some(pane.clone()),
        }]
    );
    // Also by an id the session had before a `/clear`.
    apply(
        &mut m,
        Command::SessionRekeyed {
            session_id: "s-2".into(),
            previous_ids: vec!["s-1".into()],
        },
    )
    .unwrap();
    let by_old = apply(&mut m, in_ws(resume("s-1"), &wt)).unwrap();
    assert!(matches!(&by_old[..], [Effect::Opened { pane_id: Some(p), .. }] if *p == pane));
    // A Codex thread with the same id is a different session.
    let codex = with(session(PaneKind::Codex), |c| {
        if let Command::NewSession { resume, .. } = c {
            *resume = Some("s-2".into());
        }
    });
    let (_, codex_pane, _) = new(&mut m, &main, codex);
    assert_ne!(codex_pane, pane);
}

#[test]
fn new_session_by_location_picks_the_exact_workspace_or_opens_a_transient_one() {
    let mut m = Model::default();
    let main = open(&mut m, None);
    let at = |worktree: Option<&str>| {
        with(session(PaneKind::Claude), |c| {
            if let Command::NewSession { target, .. } = c {
                *target = Target::Location {
                    project_path: PROJECT.into(),
                    worktree_path: worktree.map(Into::into),
                    project_name: None,
                    branch: Some("feat".into()),
                };
            }
        })
    };
    let effects = apply(&mut m, at(None)).unwrap();
    assert!(matches!(&effects[1], Effect::Opened { workspace_id, .. } if *workspace_id == main));
    assert_eq!(m.workspaces.len(), 1);

    // No workspace runs in the worktree: one is opened for it, not the main one.
    let effects = apply(&mut m, at(Some(WORKTREE))).unwrap();
    assert_eq!(m.workspaces.len(), 2);
    let wt = &m.workspaces[1];
    assert!(wt.transient);
    assert_eq!(wt.project_name, "app");
    assert_eq!(wt.branch.as_deref(), Some("feat"));
    assert!(matches!(&effects[0], Effect::SpawnClaude { cwd, .. } if cwd == WORKTREE));
    let tab = wt.tabs[0].id.clone();

    // Its last tab closing closes it; an ordinary workspace stays open empty.
    apply(&mut m, Command::CloseTab { tab_id: tab }).unwrap();
    assert_eq!(m.workspaces.len(), 1);
    let main_tab = m.workspaces[0].tabs[0].id.clone();
    apply(&mut m, Command::CloseTab { tab_id: main_tab }).unwrap();
    assert_eq!(m.workspaces.len(), 1);
}

#[test]
fn opening_a_transient_workspace_on_purpose_keeps_it() {
    let mut m = Model::default();
    let cmd = with(session(PaneKind::Shell), |c| {
        if let Command::NewSession { target, .. } = c {
            *target = Target::Location {
                project_path: "/repo/other/".into(),
                worktree_path: None,
                project_name: None,
                branch: None,
            };
        }
    });
    apply(&mut m, cmd).unwrap();
    assert_eq!(m.workspaces[0].project_name, "other");
    let effects = apply(
        &mut m,
        Command::OpenWorkspace {
            project_path: "/repo/other/".into(),
            project_name: "other".into(),
            worktree_path: None,
            branch: None,
            renderer: Renderer::Xterm,
        },
    )
    .unwrap();
    assert!(effects.contains(&Effect::Persist));
    assert!(!m.workspaces[0].transient);
    let tab = m.workspaces[0].tabs[0].id.clone();
    apply(&mut m, Command::CloseTab { tab_id: tab }).unwrap();
    assert_eq!(m.workspaces.len(), 1);
}

#[test]
fn commands_and_effects_use_the_camel_case_wire_shape() {
    let cmd: Command = serde_json::from_value(serde_json::json!({
        "type": "newSession",
        "projectPath": PROJECT,
        "worktreePath": WORKTREE,
        "kind": "claude",
        "resume": "s-1",
        "accountId": "work"
    }))
    .unwrap();
    let Command::NewSession {
        target, account_id, ..
    } = &cmd
    else {
        panic!("{cmd:?}")
    };
    assert!(matches!(target, Target::Location { worktree_path: Some(w), .. } if w == WORKTREE));
    assert_eq!(account_id.as_deref(), Some("work"));

    let by_ws: Command = serde_json::from_value(serde_json::json!({
        "type": "newSession", "workspaceId": "w1", "kind": "codex", "codexMode": "appServer"
    }))
    .unwrap();
    assert!(matches!(
        by_ws,
        Command::NewSession {
            target: Target::Workspace { .. },
            codex_mode: Some(CodexMode::AppServer),
            ..
        }
    ));
    let round = serde_json::to_value(&cmd).unwrap();
    assert_eq!(serde_json::from_value::<Command>(round).unwrap(), cmd);

    let effect = serde_json::to_value(Effect::End {
        pane_id: "p".into(),
        session_id: None,
    })
    .unwrap();
    assert_eq!(
        effect,
        serde_json::json!({"type": "end", "paneId": "p", "sessionId": null})
    );
}

mod layout;
mod lifecycle;
