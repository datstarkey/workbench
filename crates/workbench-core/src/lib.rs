//! workbench-core — pure (non-Tauri) logic shared by the desktop app and the
//! headless server: config & workspace persistence, git/worktree operations,
//! Claude/Codex session discovery, settings, GitHub/Trello integration, and the
//! shared serde types that define the wire contract for both the Tauri IPC layer
//! and the server's JSON API.

pub mod chat_attachment;
pub mod claude_accounts;
pub mod claude_launch;
pub mod claude_plugin;
pub mod claude_sessions;
pub mod claude_transcript;
pub mod codex_config;
pub mod codex_controls;
pub mod codex_sessions;
pub mod codex_transcript;
pub mod config;
pub mod git;
pub mod github;
mod github_api;
mod http;
pub mod net;
pub mod package_scripts;
pub mod paths;
pub mod project_files;
pub mod sandbox_runtime;
pub mod session_utils;
pub mod settings;
pub mod shell;
pub mod shell_integration;
pub mod task_output;
pub mod text;
pub mod token;
pub mod trello;
pub mod trello_automation;
pub mod types;
