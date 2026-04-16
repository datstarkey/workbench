use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use crate::paths;

use super::types::{TrelloCredentials, TrelloProjectConfig};

fn trello_config_dir() -> PathBuf {
    paths::workbench_config_dir().join("trello")
}

fn credentials_path() -> PathBuf {
    trello_config_dir().join("credentials.json")
}

fn project_config_path(project_path: &str) -> PathBuf {
    trello_config_dir()
        .join("projects")
        .join(format!("{}.json", paths::encode_project_path(project_path)))
}

pub fn load_credentials() -> Result<Option<TrelloCredentials>> {
    paths::load_json_strict(&credentials_path(), None)
}

pub fn save_credentials(creds: &TrelloCredentials) -> Result<()> {
    paths::save_json(&credentials_path(), creds)
}

pub fn delete_credentials() -> Result<()> {
    let path = credentials_path();
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn load_project_config(project_path: &str) -> Result<TrelloProjectConfig> {
    paths::load_json_strict(&project_config_path(project_path), TrelloProjectConfig::default())
}

pub fn save_project_config(project_path: &str, config: &TrelloProjectConfig) -> Result<()> {
    paths::save_json(&project_config_path(project_path), config)
}
