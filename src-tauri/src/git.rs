use std::path::Path;
use std::process::Command;

use crate::types::{BranchInfo, CreateWorktreeRequest, GitInfo, WorktreeInfo};

fn git_output(args: &[&str], cwd: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn git_info(path: &str) -> Result<GitInfo, String> {
    let branch = git_output(&["rev-parse", "--abbrev-ref", "HEAD"], path)?;
    let repo_root = git_output(&["rev-parse", "--show-toplevel"], path)?;

    // A directory is a worktree (not the main working tree) if its git common dir
    // differs from its git dir.
    let git_dir = git_output(&["rev-parse", "--git-dir"], path)?;
    let common_dir = git_output(&["rev-parse", "--git-common-dir"], path)?;
    let is_worktree = git_dir != common_dir;

    Ok(GitInfo {
        branch,
        repo_root,
        is_worktree,
    })
}

pub fn list_worktrees(path: &str) -> Result<Vec<WorktreeInfo>, String> {
    let output = git_output(&["worktree", "list", "--porcelain"], path)?;
    let mut worktrees = Vec::new();
    let mut current_path = String::new();
    let mut current_head = String::new();
    let mut current_branch = String::new();
    let mut is_bare = false;

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current_path = p.to_string();
            current_head.clear();
            current_branch.clear();
            is_bare = false;
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            current_head = h.to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            // branch refs/heads/main → main
            current_branch = b
                .strip_prefix("refs/heads/")
                .unwrap_or(b)
                .to_string();
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() && !current_path.is_empty() {
            if !is_bare {
                let is_main = worktrees.is_empty(); // first worktree is the main one
                worktrees.push(WorktreeInfo {
                    path: current_path.clone(),
                    head: current_head.clone(),
                    branch: current_branch.clone(),
                    is_main,
                });
            }
            current_path.clear();
        }
    }

    // Handle last entry (no trailing blank line)
    if !current_path.is_empty() && !is_bare {
        let is_main = worktrees.is_empty();
        worktrees.push(WorktreeInfo {
            path: current_path,
            head: current_head,
            branch: current_branch,
            is_main,
        });
    }

    Ok(worktrees)
}

pub fn create_worktree(request: &CreateWorktreeRequest) -> Result<String, String> {
    let worktree_path = if let Some(ref p) = request.path {
        p.clone()
    } else {
        // Default: sibling directory named <repo>-<branch>
        let repo = Path::new(&request.repo_path);
        let parent = repo
            .parent()
            .ok_or("Cannot determine parent directory")?;
        let repo_name = repo
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo");
        parent
            .join(format!("{}-{}", repo_name, request.branch))
            .to_string_lossy()
            .to_string()
    };

    let mut args = vec!["worktree", "add"];
    if request.new_branch {
        args.push("-b");
    }
    args.push(&request.branch);
    args.push(&worktree_path);

    // For existing branches the order is: worktree add <path> <branch>
    // For new branches: worktree add -b <branch> <path>
    // Actually git worktree add syntax is: git worktree add <path> [<branch>]
    // or: git worktree add -b <new-branch> <path> [<start-point>]
    // Let's fix the arg ordering.
    let mut args = vec!["worktree", "add"];
    if request.new_branch {
        args.extend_from_slice(&["-b", &request.branch]);
    }
    args.push(&worktree_path);
    if !request.new_branch {
        args.push(&request.branch);
    }

    git_output(&args, &request.repo_path)?;

    Ok(worktree_path)
}

pub fn remove_worktree(repo_path: &str, worktree_path: &str) -> Result<(), String> {
    git_output(&["worktree", "remove", worktree_path], repo_path)?;
    Ok(())
}

pub fn list_branches(path: &str) -> Result<Vec<BranchInfo>, String> {
    let format = "%(refname:short)\t%(objectname:short)\t%(HEAD)\t%(refname:rstrip=0)";
    let output = git_output(&["branch", "-a", &format!("--format={format}")], path)?;

    let mut branches = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() < 4 {
            continue;
        }
        let name = parts[0].to_string();
        let sha = parts[1].to_string();
        let is_current = parts[2] == "*";
        let full_ref = parts[3];
        let is_remote = full_ref.starts_with("refs/remotes/");

        // Skip HEAD pointers like origin/HEAD
        if name.ends_with("/HEAD") {
            continue;
        }

        branches.push(BranchInfo {
            name,
            sha,
            is_current,
            is_remote,
        });
    }

    Ok(branches)
}
