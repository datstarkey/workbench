use anyhow::Result;

use crate::trello::{
    api, config,
    types::{MergeAction, TrelloCredentials, TrelloProjectConfig},
};

pub fn apply_merge_action_for_branch(project_path: &str, branch: &str) -> Result<Option<String>> {
    apply_merge_action_for_branch_with(
        project_path,
        branch,
        config::load_credentials,
        config::load_project_config,
        execute_action,
    )
}

fn apply_merge_action_for_branch_with<LC, LP, EX>(
    project_path: &str,
    branch: &str,
    load_credentials: LC,
    load_project_config: LP,
    execute_action: EX,
) -> Result<Option<String>>
where
    LC: FnOnce() -> Result<Option<TrelloCredentials>>,
    LP: FnOnce(&str) -> Result<TrelloProjectConfig>,
    EX: FnOnce(&TrelloCredentials, &str, &MergeAction) -> Result<()>,
{
    let Some(creds) = load_credentials()? else {
        return Ok(None);
    };

    let project_config = load_project_config(project_path)?;
    let Some((card_id, action)) = resolve_merge_action(&project_config, branch) else {
        return Ok(None);
    };

    execute_action(&creds, &card_id, &action)?;
    Ok(Some(card_id))
}

fn resolve_merge_action(config: &TrelloProjectConfig, branch: &str) -> Option<(String, MergeAction)> {
    let link = config.task_links.iter().find(|task| task.branch == branch)?;
    let board = config.boards.iter().find(|board| board.board_id == link.board_id)?;
    let action = board.merge_action.clone()?;
    Some((link.card_id.clone(), action))
}

fn execute_action(creds: &TrelloCredentials, card_id: &str, action: &MergeAction) -> Result<()> {
    tauri::async_runtime::block_on(async {
        if let Some(target_list_id) = action.move_to_column_id.as_deref() {
            api::move_card(creds, card_id, target_list_id).await?;
        }
        for label_id in &action.add_label_ids {
            api::add_label_to_card(creds, card_id, label_id).await?;
        }
        for label_id in &action.remove_label_ids {
            api::remove_label_from_card(creds, card_id, label_id).await?;
        }
        Ok::<(), anyhow::Error>(())
    })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use anyhow::anyhow;
    use crate::trello::types::{
        BoardConfig, MergeAction, TaskLink, TrelloCredentials, TrelloProjectConfig,
    };

    use super::{apply_merge_action_for_branch_with, resolve_merge_action};

    fn config_with_merge_action() -> TrelloProjectConfig {
        TrelloProjectConfig {
            boards: vec![BoardConfig {
                board_id: "board-1".to_string(),
                board_name: "Main".to_string(),
                hidden_columns: vec![],
                link_action: None,
                merge_action: Some(MergeAction {
                    move_to_column_id: Some("done".to_string()),
                    move_to_column_name: None,
                    add_label_ids: vec!["label-1".to_string()],
                    remove_label_ids: vec![],
                }),
            }],
            task_links: vec![TaskLink {
                card_id: "card-1".to_string(),
                board_id: "board-1".to_string(),
                branch: "feature/x".to_string(),
                worktree_path: None,
                project_path: "/repo".to_string(),
            }],
        }
    }

    #[test]
    fn resolve_merge_action_returns_card_and_action_for_matching_branch() {
        let config = config_with_merge_action();

        let resolved = resolve_merge_action(&config, "feature/x");
        assert!(resolved.is_some());

        let (card_id, action) = resolved.unwrap();
        assert_eq!(card_id, "card-1");
        assert_eq!(action.move_to_column_id, Some("done".to_string()));
        assert_eq!(action.add_label_ids, vec!["label-1".to_string()]);
    }

    #[test]
    fn resolve_merge_action_returns_none_without_merge_action() {
        let config = TrelloProjectConfig {
            boards: vec![BoardConfig {
                board_id: "board-1".to_string(),
                board_name: "Main".to_string(),
                hidden_columns: vec![],
                link_action: None,
                merge_action: None,
            }],
            task_links: vec![TaskLink {
                card_id: "card-1".to_string(),
                board_id: "board-1".to_string(),
                branch: "feature/x".to_string(),
                worktree_path: None,
                project_path: "/repo".to_string(),
            }],
        };

        assert!(resolve_merge_action(&config, "feature/x").is_none());
    }

    #[test]
    fn apply_merge_action_for_branch_with_skips_when_no_credentials() {
        let load_config_called = Cell::new(false);
        let execute_called = Cell::new(false);

        let result = apply_merge_action_for_branch_with(
            "/repo",
            "feature/x",
            || Ok(None),
            |_project_path| {
                load_config_called.set(true);
                Ok(config_with_merge_action())
            },
            |_creds, _card_id, _action| {
                execute_called.set(true);
                Ok(())
            },
        )
        .unwrap();

        assert!(result.is_none());
        assert!(!load_config_called.get());
        assert!(!execute_called.get());
    }

    #[test]
    fn apply_merge_action_for_branch_with_executes_action_for_matching_branch() {
        let executed = RefCell::new((String::new(), String::new()));

        let result = apply_merge_action_for_branch_with(
            "/repo",
            "feature/x",
            || {
                Ok(Some(TrelloCredentials {
                    api_key: "k".to_string(),
                    token: "t".to_string(),
                }))
            },
            |_project_path| Ok(config_with_merge_action()),
            |_creds, card_id, action| {
                let mut data = executed.borrow_mut();
                data.0 = card_id.to_string();
                data.1 = action.move_to_column_id.clone().unwrap_or_default();
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(result, Some("card-1".to_string()));
        let data = executed.borrow();
        assert_eq!(data.0, "card-1");
        assert_eq!(data.1, "done");
    }

    #[test]
    fn apply_merge_action_for_branch_with_returns_none_when_no_matching_link_or_action() {
        let execute_called = Cell::new(false);

        let result = apply_merge_action_for_branch_with(
            "/repo",
            "feature/missing",
            || {
                Ok(Some(TrelloCredentials {
                    api_key: "k".to_string(),
                    token: "t".to_string(),
                }))
            },
            |_project_path| Ok(config_with_merge_action()),
            |_creds, _card_id, _action| {
                execute_called.set(true);
                Ok(())
            },
        )
        .unwrap();

        assert!(result.is_none());
        assert!(!execute_called.get());
    }

    #[test]
    fn apply_merge_action_for_branch_with_propagates_config_load_error() {
        let err = apply_merge_action_for_branch_with(
            "/repo",
            "feature/x",
            || {
                Ok(Some(TrelloCredentials {
                    api_key: "k".to_string(),
                    token: "t".to_string(),
                }))
            },
            |_project_path| Err(anyhow!("config load failed")),
            |_creds, _card_id, _action| Ok(()),
        )
        .unwrap_err();

        assert!(err.to_string().contains("config load failed"));
    }

    #[test]
    fn apply_merge_action_for_branch_with_propagates_execute_error() {
        let err = apply_merge_action_for_branch_with(
            "/repo",
            "feature/x",
            || {
                Ok(Some(TrelloCredentials {
                    api_key: "k".to_string(),
                    token: "t".to_string(),
                }))
            },
            |_project_path| Ok(config_with_merge_action()),
            |_creds, _card_id, _action| Err(anyhow!("execute failed")),
        )
        .unwrap_err();

        assert!(err.to_string().contains("execute failed"));
    }
}
