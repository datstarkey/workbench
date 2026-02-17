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
    let path = credentials_path();
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)?;
    let creds: TrelloCredentials = serde_json::from_str(&content)?;
    Ok(Some(creds))
}

pub fn save_credentials(creds: &TrelloCredentials) -> Result<()> {
    let content = serde_json::to_string_pretty(creds)?;
    paths::atomic_write(&credentials_path(), &content)?;
    Ok(())
}

pub fn delete_credentials() -> Result<()> {
    let path = credentials_path();
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn load_project_config(project_path: &str) -> Result<TrelloProjectConfig> {
    let path = project_config_path(project_path);
    if !path.exists() {
        return Ok(TrelloProjectConfig::default());
    }

    let content = fs::read_to_string(&path)?;
    let config: TrelloProjectConfig = serde_json::from_str(&content)?;
    Ok(config)
}

pub fn save_project_config(project_path: &str, config: &TrelloProjectConfig) -> Result<()> {
    let content = serde_json::to_string_pretty(config)?;
    paths::atomic_write(&project_config_path(project_path), &content)?;
    Ok(())
}
