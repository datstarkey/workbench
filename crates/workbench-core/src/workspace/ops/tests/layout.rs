use super::*;

#[test]
fn close_pane_closes_its_tab_with_the_last_pane() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (tab_a, pane_a, _) = new(&mut m, &ws, session(PaneKind::Shell));
    let (tab_b, pane_b, _) = new(&mut m, &ws, session(PaneKind::Shell));
    apply(
        &mut m,
        Command::MovePane {
            pane_id: pane_b.clone(),
            tab_id: tab_a.clone(),
        },
    )
    .unwrap();
    assert!(m.tab_at(&tab_b).is_none(), "the emptied tab goes");

    let effects = apply(
        &mut m,
        Command::ClosePane {
            pane_id: pane_b.clone(),
        },
    )
    .unwrap();
    assert_eq!(ends(&effects), [pane_b]);
    assert!(m.tab_at(&tab_a).is_some());
    let effects = apply(
        &mut m,
        Command::ClosePane {
            pane_id: pane_a.clone(),
        },
    )
    .unwrap();
    assert_eq!(ends(&effects), [pane_a]);
    assert!(effects.ends_with(&[Effect::Persist]));
    assert!(m.tab_at(&tab_a).is_none());
    assert!(apply(
        &mut m,
        Command::ClosePane {
            pane_id: "gone".into()
        }
    )
    .is_err());
}

#[test]
fn close_tab_workspace_and_project_end_every_pane() {
    let mut m = Model::default();
    let main = open(&mut m, None);
    let wt = open(&mut m, Some(WORKTREE));
    let (tab, pane, _) = new(&mut m, &main, session(PaneKind::Claude));
    let session_id = m.pane(&pane).unwrap().session_id.clone();
    let effects = apply(&mut m, Command::CloseTab { tab_id: tab }).unwrap();
    assert_eq!(
        effects,
        vec![
            Effect::End {
                pane_id: pane,
                session_id
            },
            Effect::Persist
        ]
    );

    let (_, a, _) = new(&mut m, &main, session(PaneKind::Shell));
    let (_, b, _) = new(&mut m, &wt, session(PaneKind::Codex));
    let effects = apply(
        &mut m,
        Command::CloseWorkspace {
            workspace_id: wt.clone(),
        },
    )
    .unwrap();
    assert_eq!(ends(&effects), [b]);
    assert!(m.workspace(&wt).is_none());

    open(&mut m, Some(WORKTREE));
    apply(
        &mut m,
        Command::OpenWorkspace {
            project_path: "/repo/other".into(),
            project_name: "other".into(),
            worktree_path: None,
            branch: None,
            renderer: Renderer::Xterm,
        },
    )
    .unwrap();
    let effects = apply(
        &mut m,
        Command::CloseProject {
            project_path: PROJECT.into(),
        },
    )
    .unwrap();
    assert_eq!(ends(&effects), [a]);
    assert_eq!(m.workspaces.len(), 1);
    assert_eq!(m.workspaces[0].project_path, "/repo/other");
    let none = apply(
        &mut m,
        Command::CloseProject {
            project_path: PROJECT.into(),
        },
    )
    .unwrap();
    assert!(none.is_empty());
}

#[test]
fn closing_a_split_tab_unsplits() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (a, _, _) = new(&mut m, &ws, session(PaneKind::Shell));
    let (b, _, _) = new(&mut m, &ws, session(PaneKind::Shell));
    apply(
        &mut m,
        Command::Split {
            tab_id: a,
            direction: SplitDirection::Vertical,
        },
    )
    .unwrap();
    assert!(m.workspace(&ws).unwrap().split_view.is_some());
    apply(&mut m, Command::CloseTab { tab_id: b }).unwrap();
    assert_eq!(m.workspace(&ws).unwrap().split_view, None);
}

#[test]
fn rename_trims_and_refuses_empty() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (tab, _, _) = new(&mut m, &ws, session(PaneKind::Claude));
    let rename = |label: &str| Command::Rename {
        tab_id: tab.clone(),
        label: label.into(),
    };
    assert_eq!(
        apply(&mut m, rename(" Fix login ")).unwrap(),
        [Effect::Persist]
    );
    assert_eq!(labels(&m, &ws), ["Fix login"]);
    assert!(apply(&mut m, rename("Fix login")).unwrap().is_empty());
    assert!(apply(&mut m, rename("  ")).is_err());
    assert!(apply(
        &mut m,
        Command::Rename {
            tab_id: "nope".into(),
            label: "x".into()
        }
    )
    .is_err());
}

#[test]
fn split_pairs_with_a_neighbour_and_toggles() {
    let mut m = Model::default();
    let ws = open(&mut m, None);
    let (a, _, _) = new(&mut m, &ws, session(PaneKind::Claude));
    let split = |tab: &str, direction| Command::Split {
        tab_id: tab.into(),
        direction,
    };

    // Alone: a new shell tab joins it.
    let effects = apply(&mut m, split(&a, SplitDirection::Horizontal)).unwrap();
    let w = m.workspace(&ws).unwrap();
    assert_eq!(w.tabs.len(), 2);
    let b = w.tabs[1].id.clone();
    assert_eq!(w.tabs[1].label, "Terminal 2");
    assert!(matches!(&effects[0], Effect::SpawnShell { .. }));
    assert_eq!(
        w.split_view,
        Some(SplitView {
            direction: SplitDirection::Horizontal,
            tab_ids: [a.clone(), b.clone()]
        })
    );
    // The other direction turns it, the same one unsplits.
    apply(&mut m, split(&b, SplitDirection::Vertical)).unwrap();
    assert_eq!(
        m.workspace(&ws)
            .unwrap()
            .split_view
            .as_ref()
            .unwrap()
            .direction,
        SplitDirection::Vertical
    );
    assert_eq!(
        apply(&mut m, split(&a, SplitDirection::Vertical)).unwrap(),
        [Effect::Persist]
    );
    assert_eq!(m.workspace(&ws).unwrap().split_view, None);

    // The last tab pairs with the one before it.
    let (c, _, _) = new(&mut m, &ws, session(PaneKind::Shell));
    let effects = apply(&mut m, split(&c, SplitDirection::Horizontal)).unwrap();
    assert_eq!(effects, [Effect::Persist]);
    assert_eq!(
        m.workspace(&ws)
            .unwrap()
            .split_view
            .as_ref()
            .unwrap()
            .tab_ids,
        [c.clone(), b.clone()]
    );
    // A split not holding the tab is replaced by one that does.
    apply(&mut m, split(&a, SplitDirection::Horizontal)).unwrap();
    assert_eq!(
        m.workspace(&ws)
            .unwrap()
            .split_view
            .as_ref()
            .unwrap()
            .tab_ids,
        [a, b]
    );
}

#[test]
fn native_workspaces_refuse_to_split() {
    let mut m = Model::default();
    let effects = apply(
        &mut m,
        Command::OpenWorkspace {
            project_path: PROJECT.into(),
            project_name: "app".into(),
            worktree_path: None,
            branch: None,
            renderer: Renderer::Native,
        },
    )
    .unwrap();
    let Effect::Opened { workspace_id, .. } = &effects[0] else {
        panic!()
    };
    let ws = workspace_id.clone();
    let (tab, _, effects) = new(&mut m, &ws, session(PaneKind::Shell));
    assert!(matches!(
        &effects[0],
        Effect::SpawnShell {
            renderer: Renderer::Native,
            ..
        }
    ));
    assert!(apply(
        &mut m,
        Command::Split {
            tab_id: tab,
            direction: SplitDirection::Horizontal
        }
    )
    .is_err());
}

#[test]
fn move_pane_stays_within_workspace_and_kind() {
    let mut m = Model::default();
    let main = open(&mut m, None);
    let wt = open(&mut m, Some(WORKTREE));
    let (a, pane_a, _) = new(&mut m, &main, session(PaneKind::Shell));
    let (b, _, _) = new(&mut m, &main, session(PaneKind::Shell));
    let (claude, _, _) = new(&mut m, &main, session(PaneKind::Claude));
    let (other, _, _) = new(&mut m, &wt, session(PaneKind::Shell));
    let mv = |tab: &str| Command::MovePane {
        pane_id: pane_a.clone(),
        tab_id: tab.into(),
    };
    assert!(apply(&mut m, mv(&other)).is_err());
    assert!(apply(&mut m, mv(&claude)).is_err());
    assert!(apply(&mut m, mv(&a)).unwrap().is_empty());
    assert_eq!(apply(&mut m, mv(&b)).unwrap(), [Effect::Persist]);
    let (_, t, _) = m.pane_at(&pane_a).unwrap();
    assert_eq!(m.workspace(&main).unwrap().tabs[t].id, b);
    assert_eq!(m.workspace(&main).unwrap().tabs[t].panes.len(), 2);
    assert!(m.tab_at(&a).is_none());
}

#[test]
fn move_tab_and_workspace_reorder() {
    let mut m = Model::default();
    let main = open(&mut m, None);
    let wt = open(&mut m, Some(WORKTREE));
    let (a, _, _) = new(&mut m, &main, session(PaneKind::Shell));
    let (b, _, _) = new(&mut m, &main, session(PaneKind::Shell));
    let (c, _, _) = new(&mut m, &main, session(PaneKind::Shell));
    let (other, _, _) = new(&mut m, &wt, session(PaneKind::Shell));
    apply(
        &mut m,
        Command::MoveTab {
            tab_id: c.clone(),
            to_tab_id: a.clone(),
        },
    )
    .unwrap();
    let ids: Vec<_> = m
        .workspace(&main)
        .unwrap()
        .tabs
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(ids, [c, a, b.clone()]);
    assert!(apply(
        &mut m,
        Command::MoveTab {
            tab_id: b,
            to_tab_id: other
        }
    )
    .is_err());
    apply(
        &mut m,
        Command::MoveWorkspace {
            workspace_id: wt.clone(),
            to_workspace_id: main.clone(),
        },
    )
    .unwrap();
    assert_eq!(m.workspaces[0].id, wt);
}

#[test]
fn trust_folder_is_for_claude_panes() {
    let mut m = Model::default();
    let ws = open(&mut m, Some(WORKTREE));
    let (_, claude, _) = new(&mut m, &ws, session(PaneKind::Claude));
    let (_, shell, _) = new(&mut m, &ws, session(PaneKind::Shell));
    assert_eq!(
        apply(
            &mut m,
            Command::TrustFolder {
                pane_id: claude.clone()
            }
        )
        .unwrap(),
        [Effect::TrustFolder {
            pane_id: claude,
            cwd: WORKTREE.into()
        }]
    );
    assert!(apply(&mut m, Command::TrustFolder { pane_id: shell }).is_err());
}
