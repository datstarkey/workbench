use crate::git;
use crate::types::{GitCommitResult, GitLogEntry, GitStashEntry, GitStatusResult};

#[tauri::command(async)]
pub fn git_status(path: String) -> Result<GitStatusResult, String> {
    git::git_status(&path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn git_log(path: String, max_count: Option<u32>) -> Result<Vec<GitLogEntry>, String> {
    git::git_log(&path, max_count.unwrap_or(50)).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn git_stage(path: String, files: Vec<String>) -> Result<bool, String> {
    git::git_stage(&path, &files).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_unstage(path: String, files: Vec<String>) -> Result<bool, String> {
    git::git_unstage(&path, &files).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_commit(path: String, message: String) -> Result<GitCommitResult, String> {
    git::git_commit(&path, &message).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn git_checkout_branch(path: String, branch: String) -> Result<bool, String> {
    git::git_checkout(&path, &branch).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_stash_list(path: String) -> Result<Vec<GitStashEntry>, String> {
    git::git_stash_list(&path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn git_stash_push(path: String, message: Option<String>) -> Result<bool, String> {
    git::git_stash_push(&path, message.as_deref()).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_stash_pop(path: String, index: u32) -> Result<bool, String> {
    git::git_stash_pop(&path, index).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_stash_drop(path: String, index: u32) -> Result<bool, String> {
    git::git_stash_drop(&path, index).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_discard_file(path: String, file: String) -> Result<bool, String> {
    git::git_discard_file(&path, &file).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_fetch(path: String) -> Result<bool, String> {
    git::git_fetch(&path).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_pull(path: String) -> Result<bool, String> {
    git::git_pull(&path).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command(async)]
pub fn git_push(path: String, set_upstream: bool) -> Result<bool, String> {
    git::git_push(&path, set_upstream).map_err(|e| e.to_string())?;
    Ok(true)
}
