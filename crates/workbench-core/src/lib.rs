//! workbench-core — pure (non-Tauri) logic shared by the desktop app and the
//! headless server: config & workspace persistence, git/worktree operations,
//! Claude/Codex session discovery, settings, GitHub/Trello integration, and the
//! shared serde types that define the wire contract for both the Tauri IPC layer
//! and the server's JSON API.

pub mod claude_sessions;
pub mod codex_config;
pub mod codex_sessions;
pub mod config;
pub mod git;
pub mod github;
pub mod paths;
pub mod session_utils;
pub mod settings;
pub mod shell_integration;
pub mod trello;
pub mod trello_automation;
pub mod types;
