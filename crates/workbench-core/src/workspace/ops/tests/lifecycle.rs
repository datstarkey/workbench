use super::*;

#[test]
fn restart_keeps_ids_session_account_and_prompt() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let cmd = with(session(PaneKind::Claude), |c| {
        if let Command::NewSession {
            account_id, prompt, ..
        } = c
        {
            *account_id = Some("work".into());
            *prompt = Some("go".into());
        }
    });
    let (tab, pane, _) = new(&mut m, &ws, cmd);
    let before = m.clone();
    let session_id = m.pane(&pane).unwrap().session_id.clone().unwrap();
    let effects = apply(
        &mut m,
        Command::Restart {
            tab_id: tab.clone(),
        },
    )
    .unwrap();
    assert_eq!(m, before, "a restart changes nothing in the model");
    assert_eq!(
        effects,
        vec![
            Effect::Stop {
                pane_id: pane.clone(),
                session_id: Some(session_id.clone()),
            },
            Effect::SpawnClaude {
                pane_id: pane,
                cwd: PROJECT.into(),
                renderer: Renderer::Xterm,
                session_id,
                resume: true,
                account_id: Some("work".into()),
                prompt: Some("go".into()),
            },
            Effect::Opened {
                workspace_id: ws.clone(),
                tab_id: Some(tab),
                pane_id: None,
            },
        ]
    );
    let (shell, _, _) = new(&mut m, &ws, session(PaneKind::Shell));
    assert!(apply(&mut m, Command::Restart { tab_id: shell }).is_err());
}

#[test]
fn restart_gives_a_claude_pane_without_an_id_a_new_session() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (tab, pane, _) = new(&mut m, &ws, session(PaneKind::Claude));
    let (w, t, p) = m.pane_at(&pane).unwrap();
    m.workspaces[w].tabs[t].panes[p].session_id = None;
    let effects = apply(&mut m, Command::Restart { tab_id: tab }).unwrap();
    let id = m.pane(&pane).unwrap().session_id.clone().unwrap();
    assert!(effects.contains(&Effect::Persist));
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::SpawnClaude { resume: false, session_id, .. } if *session_id == id
    )));
}

#[test]
fn restart_codex_resumes_its_thread_in_its_mode() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let cmd = with(session(PaneKind::Codex), |c| {
        if let Command::NewSession { codex_mode, .. } = c {
            *codex_mode = Some(CodexMode::AppServer);
        }
    });
    let (tab, pane, _) = new(&mut m, &ws, cmd);
    apply(
        &mut m,
        Command::SessionAttached {
            pane_id: pane.clone(),
            session_id: "t-1".into(),
        },
    )
    .unwrap();
    let effects = apply(&mut m, Command::Restart { tab_id: tab }).unwrap();
    assert!(effects.contains(&Effect::SpawnCodex {
        pane_id: pane,
        cwd: PROJECT.into(),
        renderer: Renderer::Xterm,
        session_id: Some("t-1".into()),
        mode: CodexMode::AppServer,
        prompt: None,
    }));
}

#[test]
fn codex_mode_switch_stops_then_starts_the_other_process() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (_, pane, _) = new(&mut m, &ws, session(PaneKind::Codex));
    apply(
        &mut m,
        Command::SessionAttached {
            pane_id: pane.clone(),
            session_id: "t-1".into(),
        },
    )
    .unwrap();
    let to_chat = apply(
        &mut m,
        Command::SetCodexMode {
            pane_id: pane.clone(),
            mode: CodexMode::AppServer,
        },
    )
    .unwrap();
    let spawn = |mode| Effect::SpawnCodex {
        pane_id: pane.clone(),
        cwd: PROJECT.into(),
        renderer: Renderer::Xterm,
        session_id: Some("t-1".into()),
        mode,
        prompt: None,
    };
    let stop = Effect::Stop {
        pane_id: pane.clone(),
        session_id: Some("t-1".into()),
    };
    assert_eq!(
        to_chat,
        vec![stop.clone(), spawn(CodexMode::AppServer), Effect::Persist]
    );
    assert_eq!(
        m.pane(&pane).unwrap().codex_mode,
        Some(CodexMode::AppServer)
    );
    let same = apply(
        &mut m,
        Command::SetCodexMode {
            pane_id: pane.clone(),
            mode: CodexMode::AppServer,
        },
    )
    .unwrap();
    assert!(same.is_empty());
    let back = apply(
        &mut m,
        Command::SetCodexMode {
            pane_id: pane.clone(),
            mode: CodexMode::Tui,
        },
    )
    .unwrap();
    assert_eq!(back, vec![stop, spawn(CodexMode::Tui), Effect::Persist]);

    let (_, claude, _) = new(&mut m, &ws, session(PaneKind::Claude));
    assert!(apply(
        &mut m,
        Command::SetCodexMode {
            pane_id: claude,
            mode: CodexMode::Tui
        }
    )
    .is_err());
}

#[test]
fn a_codex_prompt_is_only_sent_to_a_new_thread() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let cmd = with(session(PaneKind::Codex), |c| {
        if let Command::NewSession { prompt, .. } = c {
            *prompt = Some("review".into());
        }
    });
    let (tab, pane, _) = new(&mut m, &ws, cmd);
    apply(
        &mut m,
        Command::SessionAttached {
            pane_id: pane,
            session_id: "t-1".into(),
        },
    )
    .unwrap();
    let effects = apply(&mut m, Command::Restart { tab_id: tab }).unwrap();
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::SpawnCodex { prompt: None, .. })));
}

#[test]
fn rekey_follows_clear_and_attach_switches_conversation() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (_, pane, _) = new(&mut m, &ws, session(PaneKind::Claude));
    let first = m.pane(&pane).unwrap().session_id.clone().unwrap();
    let rekey = |id: &str, prev: &[&str]| Command::SessionRekeyed {
        session_id: id.into(),
        previous_ids: prev.iter().map(|s| s.to_string()).collect(),
    };
    assert_eq!(
        apply(&mut m, rekey("s-2", &[&first])).unwrap(),
        [Effect::Persist]
    );
    assert_eq!(
        apply(&mut m, rekey("s-3", &["s-2", &first])).unwrap(),
        [Effect::Persist]
    );
    let p = m.pane(&pane).unwrap();
    assert_eq!(p.session_id.as_deref(), Some("s-3"));
    assert_eq!(p.previous_ids, [first.clone(), "s-2".to_string()]);
    // Repeats and unknown sessions change nothing.
    assert!(apply(&mut m, rekey("s-3", &["s-2"])).unwrap().is_empty());
    assert!(apply(&mut m, rekey("x-2", &["x-1"])).unwrap().is_empty());

    // `/resume` to another conversation drops the old ids.
    let attach = |id: &str| Command::SessionAttached {
        pane_id: pane.clone(),
        session_id: id.into(),
    };
    assert_eq!(apply(&mut m, attach("other")).unwrap(), [Effect::Persist]);
    let p = m.pane(&pane).unwrap();
    assert_eq!(p.session_id.as_deref(), Some("other"));
    assert!(p.previous_ids.is_empty());
    assert!(apply(&mut m, attach("other")).unwrap().is_empty());
    // A closed pane's late event is ignored.
    assert!(apply(
        &mut m,
        Command::SessionAttached {
            pane_id: "gone".into(),
            session_id: "x".into()
        }
    )
    .unwrap()
    .is_empty());
}

#[test]
fn an_exit_keeps_the_pane() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (_, pane, _) = new(&mut m, &ws, session(PaneKind::Claude));
    let before = m.clone();
    assert!(apply(&mut m, Command::PaneExited { pane_id: pane })
        .unwrap()
        .is_empty());
    assert_eq!(m, before);
}
