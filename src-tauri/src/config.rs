use anyhow::Result;
use std::fs;
use std::io::BufRead;
use std::path::PathBuf;

use crate::paths;
use crate::types::{DiscoveredClaudeSession, ProjectConfig, ProjectsFile, WorkspaceFile};

fn config_path() -> PathBuf {
    paths::workbench_config_dir().join("projects.json")
}

pub fn load_projects() -> Result<Vec<ProjectConfig>> {
    let path = config_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path)?;
    let file: ProjectsFile = serde_json::from_str(&content)?;
    Ok(file.projects)
}

pub fn save_projects(projects: &[ProjectConfig]) -> Result<()> {
    let file = ProjectsFile {
        projects: projects.to_vec(),
    };
    let content = serde_json::to_string_pretty(&file)?;
    paths::atomic_write(&config_path(), &content)?;
    Ok(())
}

fn workspace_path() -> PathBuf {
    paths::workbench_config_dir().join("workspaces.json")
}

pub fn load_workspaces() -> Result<WorkspaceFile> {
    let path = workspace_path();
    if !path.exists() {
        return Ok(WorkspaceFile {
            workspaces: Vec::new(),
            selected_id: None,
        });
    }

    let content = fs::read_to_string(&path)?;
    let file: WorkspaceFile = serde_json::from_str(&content)?;
    Ok(file)
}

pub fn save_workspaces(file: &WorkspaceFile) -> Result<()> {
    let content = serde_json::to_string_pretty(file)?;
    paths::atomic_write(&workspace_path(), &content)?;
    Ok(())
}

/// Encode a project path the same way Claude CLI does: replace `/` with `-`
fn encode_project_path(project_path: &str) -> String {
    project_path.replace('/', "-")
}

/// Parse a single JSONL session file into a DiscoveredClaudeSession.
fn parse_session_jsonl(path: &std::path::Path, session_id: String) -> Option<DiscoveredClaudeSession> {
    let file = fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);

    let mut label = String::new();
    let mut timestamp = String::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let obj: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Grab timestamp from first entry
        if timestamp.is_empty() {
            if let Some(ts) = obj.get("timestamp").and_then(|v| v.as_str()) {
                timestamp = ts.to_string();
            }
        }

        // Find first real user message text
        if obj.get("type").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        // Skip meta/system messages
        if obj.get("isMeta").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }

        let content = obj
            .get("message")
            .and_then(|m| m.get("content"));

        let text = match content {
            // content can be a plain string or an array of content blocks
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Array(arr)) => {
                arr.iter().find_map(|item| {
                    if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                        item.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                    } else {
                        None
                    }
                })
            }
            _ => None,
        };

        if let Some(raw) = text {
            let trimmed = raw.trim();
            // Skip system/tool/command messages
            if trimmed.is_empty()
                || trimmed.len() <= 5
                || trimmed.starts_with('<')
                || trimmed.starts_with("[Request interrupted")
                || trimmed.starts_with("Base directory")
            {
                continue;
            }
            // Truncate to first line or 80 chars
            let first_line = trimmed.lines().next().unwrap_or(trimmed);
            label = if first_line.len() > 80 {
                format!("{}...", &first_line[..77])
            } else {
                first_line.to_string()
            };
            break;
        }
    }

    if label.is_empty() {
        label = format!("Session {}", &session_id[..8.min(session_id.len())]);
    }

    Some(DiscoveredClaudeSession {
        session_id,
        label,
        timestamp,
        last_message_role: None,
    })
}

/// Discover Claude CLI sessions by reading ~/.claude/projects/<encoded-path>/*.jsonl
pub fn discover_claude_sessions(project_path: &str) -> Result<Vec<DiscoveredClaudeSession>> {
    let encoded = encode_project_path(project_path);
    let sessions_dir = paths::claude_user_dir().join("projects").join(&encoded);

    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let session_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if session_id.is_empty() {
            continue;
        }

        if let Some(session) = parse_session_jsonl(&path, session_id) {
            sessions.push(session);
        }
    }

    // Sort by timestamp descending (newest first)
    sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Ok(sessions)
}
