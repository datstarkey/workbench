use serde::{Deserialize, Serialize};

// API response types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloCredentials {
    pub api_key: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloBoard {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrelloOrganization {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloList {
    pub id: String,
    pub name: String,
    pub pos: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloCard {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub id_list: String,
    pub url: String,
    pub labels: Vec<TrelloLabel>,
    pub pos: f64,
    pub due: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloLabel {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloBoardData {
    pub board: TrelloBoard,
    pub columns: Vec<TrelloColumnData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrelloColumnData {
    pub column: TrelloList,
    pub cards: Vec<TrelloCard>,
}

// Config types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardConfig {
    pub board_id: String,
    pub board_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_columns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_action: Option<MergeAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_action: Option<MergeAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeAction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_to_column_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_to_column_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_label_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_label_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLink {
    pub card_id: String,
    pub board_id: String,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    pub project_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrelloProjectConfig {
    #[serde(default)]
    pub boards: Vec<BoardConfig>,
    #[serde(default)]
    pub task_links: Vec<TaskLink>,
}
