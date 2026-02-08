use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::types::{BranchInfo, CreateWorktreeRequest, GitInfo, WorktreeCopyOptions, WorktreeInfo};

fn git_output(args: &[&str], cwd: &str) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("Failed to run git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("{stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn list_ignored_paths(repo_root: &str) -> Result<Vec<String>> {
    let output = git_output(
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--full-name",
        ],
        repo_root,
    )?;

    Ok(output
        .lines()
        .map(|line| line.trim().trim_end_matches('/').to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && !path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn is_relevant_workspace_ignored_path(rel_path: &str, options: &WorktreeCopyOptions) -> bool {
    let normalized = rel_path.trim_end_matches('/');
    if normalized.is_empty() {
        return false;
    }

    if options.ai_config {
        if normalized == ".claude" || normalized.starts_with(".claude/") {
            return true;
        }
        if normalized == ".codex" || normalized.starts_with(".codex/") {
            return true;
        }
    }

    let file_name = Path::new(normalized)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    options.env_files
        && (file_name == ".env"
            || file_name.starts_with(".env.")
            || file_name == ".envrc"
            || file_name == ".dev.vars")
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let source_path = entry.path();
        let dest_path = dst.join(entry.file_name());

        copy_path_recursive(&source_path, &dest_path)?;
    }

    Ok(())
}

fn copy_path_recursive(src: &Path, dst: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(src)?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Ok(());
    }

    if file_type.is_dir() {
        copy_dir_recursive(src, dst)
    } else if file_type.is_file() {
        copy_file(src, dst)
    } else {
        Ok(())
    }
}

fn collect_workspace_copy_candidates(
    repo_root: &Path,
    options: &WorktreeCopyOptions,
) -> BTreeSet<PathBuf> {
    let mut candidates = BTreeSet::new();
    if options.ai_config {
        candidates.insert(PathBuf::from(".claude"));
        candidates.insert(PathBuf::from(".codex"));
    }

    if options.env_files {
        let env_default_paths = [
            ".env",
            ".env.local",
            ".env.development",
            ".env.production",
            ".env.test",
            ".envrc",
            ".dev.vars",
        ];

        for path in env_default_paths {
            candidates.insert(PathBuf::from(path));
        }
    }

    if options.env_files {
        if let Ok(entries) = fs::read_dir(repo_root) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let Some(name) = file_name.to_str() else {
                    continue;
                };

                if name.starts_with(".env.") {
                    candidates.insert(PathBuf::from(name));
                }
            }
        }
    }

    if options.ai_config || options.env_files {
        if let Ok(ignored_paths) = list_ignored_paths(&repo_root.to_string_lossy()) {
            for rel_path in ignored_paths {
                if is_relevant_workspace_ignored_path(&rel_path, options) {
                    candidates.insert(PathBuf::from(rel_path));
                }
            }
        }
    }

    candidates
}

fn copy_workspace_files_to_worktree(
    repo_root: &Path,
    worktree_path: &Path,
    options: &WorktreeCopyOptions,
) -> Result<()> {
    let candidates = collect_workspace_copy_candidates(repo_root, options);

    for relative in candidates {
        if !is_safe_relative_path(&relative) {
            continue;
        }

        let source = repo_root.join(&relative);
        if !source.exists() {
            continue;
        }

        let destination = worktree_path.join(&relative);
        if destination.exists() {
            continue;
        }

        copy_path_recursive(&source, &destination)
            .with_context(|| format!("Failed to copy {}", relative.display()))?;
    }

    Ok(())
}

pub fn git_info(path: &str) -> Result<GitInfo> {
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

pub fn list_worktrees(path: &str) -> Result<Vec<WorktreeInfo>> {
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
            current_branch = b.strip_prefix("refs/heads/").unwrap_or(b).to_string();
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() && !current_path.is_empty() {
            if !is_bare {
                let is_main = worktrees.is_empty();
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

pub fn create_worktree(request: &CreateWorktreeRequest) -> Result<String> {
    let repo_root = git_output(&["rev-parse", "--show-toplevel"], &request.repo_path)?;
    let copy_options = request.copy_options.clone().unwrap_or_default();

    let worktree_path = if let Some(ref p) = request.path {
        p.clone()
    } else {
        let repo = Path::new(&request.repo_path);
        let parent = repo.parent().context("Cannot determine parent directory")?;
        let repo_name = repo.file_name().and_then(|n| n.to_str()).unwrap_or("repo");
        parent
            .join(format!("{}-{}", repo_name, request.branch))
            .to_string_lossy()
            .to_string()
    };

    // git worktree add syntax:
    //   new branch:      git worktree add -b <branch> <path>
    //   existing branch:  git worktree add <path> <branch>
    let mut args = vec!["worktree", "add"];
    if request.new_branch {
        args.extend_from_slice(&["-b", &request.branch]);
    }
    args.push(&worktree_path);
    if !request.new_branch {
        args.push(&request.branch);
    }

    git_output(&args, &request.repo_path)?;
    if copy_options.ai_config || copy_options.env_files {
        copy_workspace_files_to_worktree(
            Path::new(&repo_root),
            Path::new(&worktree_path),
            &copy_options,
        )?;
    }

    Ok(worktree_path)
}

pub fn remove_worktree(repo_path: &str, worktree_path: &str) -> Result<()> {
    git_output(&["worktree", "remove", worktree_path], repo_path)?;
    Ok(())
}

pub fn list_branches(path: &str) -> Result<Vec<BranchInfo>> {
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
