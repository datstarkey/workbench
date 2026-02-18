use anyhow::Result;

use crate::trello::{
    api, config,
    types::{MergeAction, TrelloCredentials, TrelloProjectConfig},
};

pub fn apply_merge_action_for_branch(project_path: &str, branch: &str) -> Result<Option<String>> {
    let Some(creds) = config::load_credentials()? else {
        return Ok(None);
    };

    let project_config = config::load_project_config(project_path)?;
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
    use crate::trello::types::{BoardConfig, MergeAction, TaskLink, TrelloProjectConfig};

    use super::resolve_merge_action;

    #[test]
    fn resolve_merge_action_returns_card_and_action_for_matching_branch() {
        let config = TrelloProjectConfig {
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
        };

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
}
