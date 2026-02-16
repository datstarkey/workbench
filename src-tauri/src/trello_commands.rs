use crate::trello::{
    api, config,
    types::*,
};

fn load_creds() -> Result<TrelloCredentials, String> {
    config::load_credentials()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Not authenticated with Trello".to_string())
}

// Auth commands (take explicit credentials)

#[tauri::command(async)]
pub async fn trello_validate_auth(api_key: String, token: String) -> Result<bool, String> {
    let creds = TrelloCredentials {
        api_key,
        token,
    };
    api::validate_auth(&creds).await.map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub async fn trello_list_boards(
    api_key: String,
    token: String,
) -> Result<Vec<TrelloBoard>, String> {
    let creds = TrelloCredentials {
        api_key,
        token,
    };
    api::list_boards(&creds).await.map_err(|e| e.to_string())
}

// API commands (load stored credentials)

#[tauri::command(async)]
pub async fn trello_fetch_board_data(
    board_id: String,
    hidden_columns: Vec<String>,
) -> Result<TrelloBoardData, String> {
    let creds = load_creds()?;
    api::fetch_board_data(&creds, &board_id, &hidden_columns)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub async fn trello_list_columns(board_id: String) -> Result<Vec<TrelloList>, String> {
    let creds = load_creds()?;
    api::list_columns(&creds, &board_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub async fn trello_list_labels(board_id: String) -> Result<Vec<TrelloLabel>, String> {
    let creds = load_creds()?;
    api::list_labels(&creds, &board_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub async fn trello_create_card(
    list_id: String,
    name: String,
    description: Option<String>,
) -> Result<TrelloCard, String> {
    let creds = load_creds()?;
    api::create_card(&creds, &list_id, &name, description.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub async fn trello_move_card(
    card_id: String,
    target_list_id: String,
) -> Result<bool, String> {
    let creds = load_creds()?;
    api::move_card(&creds, &card_id, &target_list_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub async fn trello_add_label(card_id: String, label_id: String) -> Result<bool, String> {
    let creds = load_creds()?;
    api::add_label_to_card(&creds, &card_id, &label_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub async fn trello_remove_label(card_id: String, label_id: String) -> Result<bool, String> {
    let creds = load_creds()?;
    api::remove_label_from_card(&creds, &card_id, &label_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(true)
}

// Config commands

#[tauri::command(async)]
pub fn trello_load_credentials() -> Result<Option<TrelloCredentials>, String> {
    config::load_credentials().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn trello_save_credentials(api_key: String, token: String) -> Result<bool, String> {
    let creds = TrelloCredentials {
        api_key,
        token,
    };
    config::save_credentials(&creds).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn trello_disconnect() -> Result<bool, String> {
    config::delete_credentials().map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn trello_load_project_config(
    project_path: String,
) -> Result<TrelloProjectConfig, String> {
    config::load_project_config(&project_path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn trello_save_project_config(
    project_path: String,
    config: TrelloProjectConfig,
) -> Result<bool, String> {
    config::save_project_config(&project_path, &config).map_err(|e| e.to_string())?;
    Ok(true)
}
