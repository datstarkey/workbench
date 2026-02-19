use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::paths::{copy_dir_skip_symlinks, copy_file};
use crate::types::{
    BranchInfo, CreateWorktreeRequest, GitCommitResult, GitFileStatus, GitInfo, GitLogEntry,
    GitStashEntry, GitStatusResult, WorktreeCopyOptions, WorktreeInfo,
};

pub(crate) fn git_output(args: &[&str], cwd: &str) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("PATH", crate::paths::enriched_path())
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

pub(crate) fn is_safe_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && !path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

pub(crate) fn is_relevant_workspace_ignored_path(rel_path: &str, options: &WorktreeCopyOptions) -> bool {
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
        if normalized == ".mcp.json" {
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

fn collect_workspace_copy_candidates(
    repo_root: &Path,
    options: &WorktreeCopyOptions,
) -> BTreeSet<PathBuf> {
    let mut candidates = BTreeSet::new();
    if options.ai_config {
        candidates.insert(PathBuf::from(".claude"));
        candidates.insert(PathBuf::from(".codex"));
        candidates.insert(PathBuf::from(".mcp.json"));
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

        let meta = fs::symlink_metadata(&source)?;
        if meta.file_type().is_symlink() {
            continue;
        } else if meta.is_dir() {
            copy_dir_skip_symlinks(&source, &destination)
                .with_context(|| format!("Failed to copy {}", relative.display()))?;
        } else if meta.is_file() {
            copy_file(&source, &destination)
                .with_context(|| format!("Failed to copy {}", relative.display()))?;
        }
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

pub(crate) fn parse_worktree_porcelain(output: &str) -> Vec<WorktreeInfo> {
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

    worktrees
}

pub fn list_worktrees(path: &str) -> Result<Vec<WorktreeInfo>> {
    let output = git_output(&["worktree", "list", "--porcelain"], path)?;
    Ok(parse_worktree_porcelain(&output))
}

pub fn create_worktree(request: &CreateWorktreeRequest) -> Result<String> {
    let repo_root = git_output(&["rev-parse", "--show-toplevel"], &request.repo_path)?;
    let copy_options = request.copy_options.clone().unwrap_or_default();
    let strategy = request.strategy.as_deref().unwrap_or("sibling");

    let worktree_path = if let Some(ref p) = request.path {
        p.clone()
    } else if strategy == "inside" {
        Path::new(&repo_root)
            .join(".worktrees")
            .join(&request.branch)
            .to_string_lossy()
            .to_string()
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

    if strategy == "inside" {
        let gitignore_path = Path::new(&repo_root).join(".gitignore");
        ensure_gitignore_entry(&gitignore_path, ".worktrees/")?;
    }

    if copy_options.ai_config || copy_options.env_files {
        copy_workspace_files_to_worktree(
            Path::new(&repo_root),
            Path::new(&worktree_path),
            &copy_options,
        )?;
    }

    Ok(worktree_path)
}

/// Append an entry to a .gitignore file if it's not already present.
fn ensure_gitignore_entry(gitignore_path: &Path, entry: &str) -> Result<()> {
    let content = fs::read_to_string(gitignore_path).unwrap_or_default();
    let already_present = content
        .lines()
        .any(|line| line.trim() == entry.trim());
    if !already_present {
        let mut new_content = content;
        if !new_content.is_empty() && !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(entry);
        new_content.push('\n');
        fs::write(gitignore_path, new_content)?;
    }
    Ok(())
}

pub fn remove_worktree(repo_path: &str, worktree_path: &str, force: bool) -> Result<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(worktree_path);
    git_output(&args, repo_path)?;
    Ok(())
}

pub fn delete_branch(repo_path: &str, branch: &str, force: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    git_output(&["branch", flag, branch], repo_path)?;
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

// --- Git sidebar operations ---

pub(crate) fn parse_porcelain_status(output: &str) -> Vec<GitFileStatus> {
    let mut files = Vec::new();
    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }
        let x = line.as_bytes()[0];
        let y = line.as_bytes()[1];
        let path = line[3..].to_string();

        let staged = x != b' ' && x != b'?';
        let unstaged = y != b' ' && y != b'?';

        let status = match (x, y) {
            (b'?', b'?') => "untracked".to_string(),
            (b'!', b'!') => "ignored".to_string(),
            (b'A', _) | (_, b'A') if x != b'D' && y != b'D' => "added".to_string(),
            (b'D', _) | (_, b'D') => "deleted".to_string(),
            (b'R', _) => "renamed".to_string(),
            (b'C', _) => "copied".to_string(),
            _ => "modified".to_string(),
        };

        files.push(GitFileStatus {
            path,
            status,
            staged,
            unstaged,
        });
    }
    files
}

pub fn git_status(path: &str) -> Result<GitStatusResult> {
    let branch = git_output(&["rev-parse", "--abbrev-ref", "HEAD"], path)
        .unwrap_or_else(|_| "HEAD".to_string());

    let porcelain = git_output(&["status", "--porcelain=v1"], path)?;
    let files = parse_porcelain_status(&porcelain);

    // ahead/behind — may fail if there's no upstream
    let (ahead, behind, has_upstream) = match git_output(
        &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
        path,
    ) {
        Ok(output) => {
            let parts: Vec<&str> = output.split_whitespace().collect();
            if parts.len() == 2 {
                let behind_count = parts[0].parse::<u32>().unwrap_or(0);
                let ahead_count = parts[1].parse::<u32>().unwrap_or(0);
                (ahead_count, behind_count, true)
            } else {
                (0, 0, true)
            }
        }
        Err(_) => (0, 0, false),
    };

    Ok(GitStatusResult {
        branch,
        files,
        ahead,
        behind,
        has_upstream,
    })
}

pub fn git_log(path: &str, max_count: u32) -> Result<Vec<GitLogEntry>> {
    let format = "%H%x00%h%x00%s%x00%an%x00%aI";
    let count_arg = format!("-{}", max_count);
    let output = git_output(
        &["log", &format!("--format={format}"), &count_arg],
        path,
    )?;

    let mut entries = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(5, '\0').collect();
        if parts.len() < 5 {
            continue;
        }
        entries.push(GitLogEntry {
            sha: parts[0].to_string(),
            short_sha: parts[1].to_string(),
            message: parts[2].to_string(),
            author: parts[3].to_string(),
            date: parts[4].to_string(),
        });
    }
    Ok(entries)
}

pub fn git_stage(path: &str, files: &[String]) -> Result<()> {
    let mut args: Vec<&str> = vec!["add", "--"];
    for f in files {
        args.push(f.as_str());
    }
    git_output(&args, path)?;
    Ok(())
}

pub fn git_unstage(path: &str, files: &[String]) -> Result<()> {
    let mut args: Vec<&str> = vec!["restore", "--staged", "--"];
    for f in files {
        args.push(f.as_str());
    }
    git_output(&args, path)?;
    Ok(())
}

pub fn git_commit(path: &str, message: &str) -> Result<GitCommitResult> {
    git_output(&["commit", "-m", message], path)?;
    let sha = git_output(&["rev-parse", "--short", "HEAD"], path)?;
    Ok(GitCommitResult {
        sha,
        message: message.to_string(),
    })
}

pub fn git_checkout(path: &str, branch: &str) -> Result<()> {
    git_output(&["checkout", branch], path)?;
    Ok(())
}

pub fn git_stash_list(path: &str) -> Result<Vec<GitStashEntry>> {
    let output = git_output(
        &["stash", "list", "--format=%gd%x00%gs%x00%aI"],
        path,
    );

    // Empty stash list returns an error from git_output because there's no output
    let output = match output {
        Ok(o) => o,
        Err(_) => return Ok(Vec::new()),
    };

    let mut entries = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(3, '\0').collect();
        if parts.len() < 3 {
            continue;
        }
        // parts[0] is like "stash@{0}"
        let index = parts[0]
            .strip_prefix("stash@{")
            .and_then(|s| s.strip_suffix('}'))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        entries.push(GitStashEntry {
            index,
            message: parts[1].to_string(),
            date: parts[2].to_string(),
        });
    }
    Ok(entries)
}

pub fn git_stash_push(path: &str, message: Option<&str>) -> Result<()> {
    let mut args = vec!["stash", "push"];
    if let Some(msg) = message {
        args.extend_from_slice(&["-m", msg]);
    }
    git_output(&args, path)?;
    Ok(())
}

pub fn git_stash_pop(path: &str, index: u32) -> Result<()> {
    let stash_ref = format!("stash@{{{index}}}");
    git_output(&["stash", "pop", &stash_ref], path)?;
    Ok(())
}

pub fn git_stash_drop(path: &str, index: u32) -> Result<()> {
    let stash_ref = format!("stash@{{{index}}}");
    git_output(&["stash", "drop", &stash_ref], path)?;
    Ok(())
}

pub fn git_discard_file(path: &str, file: &str) -> Result<()> {
    git_output(&["checkout", "--", file], path)?;
    Ok(())
}

pub fn git_fetch(path: &str) -> Result<()> {
    git_output(&["fetch"], path)?;
    Ok(())
}

pub fn git_pull(path: &str) -> Result<()> {
    git_output(&["pull"], path)?;
    Ok(())
}

pub fn git_push(path: &str, set_upstream: bool) -> Result<()> {
    if set_upstream {
        let branch = git_output(&["rev-parse", "--abbrev-ref", "HEAD"], path)?;
        git_output(&["push", "-u", "origin", &branch], path)?;
    } else {
        git_output(&["push"], path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WorktreeCopyOptions;
    use std::path::Path;

    // --- is_safe_relative_path ---

    #[test]
    fn safe_relative_path_normal() {
        assert!(is_safe_relative_path(Path::new("foo/bar")));
    }

    #[test]
    fn safe_relative_path_dotfile() {
        assert!(is_safe_relative_path(Path::new(".env")));
    }

    #[test]
    fn safe_relative_path_parent_dir_rejected() {
        assert!(!is_safe_relative_path(Path::new("../escape")));
    }

    #[test]
    fn safe_relative_path_absolute_rejected() {
        assert!(!is_safe_relative_path(Path::new("/absolute/path")));
    }

    #[test]
    fn safe_relative_path_empty() {
        assert!(is_safe_relative_path(Path::new("")));
    }

    // --- is_relevant_workspace_ignored_path ---

    #[test]
    fn ignored_path_ai_config_claude_dir() {
        let opts = WorktreeCopyOptions { ai_config: true, env_files: false };
        assert!(is_relevant_workspace_ignored_path(".claude", &opts));
    }

    #[test]
    fn ignored_path_ai_config_claude_subpath() {
        let opts = WorktreeCopyOptions { ai_config: true, env_files: false };
        assert!(is_relevant_workspace_ignored_path(".claude/settings.json", &opts));
    }

    #[test]
    fn ignored_path_ai_config_codex() {
        let opts = WorktreeCopyOptions { ai_config: true, env_files: false };
        assert!(is_relevant_workspace_ignored_path(".codex", &opts));
    }

    #[test]
    fn ignored_path_ai_config_mcp_json() {
        let opts = WorktreeCopyOptions { ai_config: true, env_files: false };
        assert!(is_relevant_workspace_ignored_path(".mcp.json", &opts));
    }

    #[test]
    fn ignored_path_ai_config_unrelated_file() {
        let opts = WorktreeCopyOptions { ai_config: true, env_files: false };
        assert!(!is_relevant_workspace_ignored_path("src/main.rs", &opts));
    }

    #[test]
    fn ignored_path_env_files_env() {
        let opts = WorktreeCopyOptions { ai_config: false, env_files: true };
        assert!(is_relevant_workspace_ignored_path(".env", &opts));
    }

    #[test]
    fn ignored_path_env_files_env_local() {
        let opts = WorktreeCopyOptions { ai_config: false, env_files: true };
        assert!(is_relevant_workspace_ignored_path(".env.local", &opts));
    }

    #[test]
    fn ignored_path_env_files_envrc() {
        let opts = WorktreeCopyOptions { ai_config: false, env_files: true };
        assert!(is_relevant_workspace_ignored_path(".envrc", &opts));
    }

    #[test]
    fn ignored_path_env_files_dev_vars() {
        let opts = WorktreeCopyOptions { ai_config: false, env_files: true };
        assert!(is_relevant_workspace_ignored_path(".dev.vars", &opts));
    }

    #[test]
    fn ignored_path_env_files_unrelated() {
        let opts = WorktreeCopyOptions { ai_config: false, env_files: true };
        assert!(!is_relevant_workspace_ignored_path("package.json", &opts));
    }

    #[test]
    fn ignored_path_both_disabled() {
        let opts = WorktreeCopyOptions { ai_config: false, env_files: false };
        assert!(!is_relevant_workspace_ignored_path(".claude", &opts));
        assert!(!is_relevant_workspace_ignored_path(".env", &opts));
    }

    #[test]
    fn ignored_path_empty_string() {
        let opts = WorktreeCopyOptions { ai_config: true, env_files: true };
        assert!(!is_relevant_workspace_ignored_path("", &opts));
    }

    #[test]
    fn ignored_path_trailing_slash_stripped() {
        let opts = WorktreeCopyOptions { ai_config: true, env_files: false };
        assert!(is_relevant_workspace_ignored_path(".claude/", &opts));
    }

    // --- parse_worktree_porcelain ---

    #[test]
    fn parse_worktree_single_main() {
        let output = "worktree /home/user/repo\nHEAD abc1234\nbranch refs/heads/main\n\n";
        let result = parse_worktree_porcelain(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "/home/user/repo");
        assert_eq!(result[0].branch, "main");
        assert!(result[0].is_main);
    }

    #[test]
    fn parse_worktree_main_plus_secondary() {
        let output = "\
worktree /home/user/repo
HEAD abc1234
branch refs/heads/main

worktree /home/user/repo-feature
HEAD def5678
branch refs/heads/feature

";
        let result = parse_worktree_porcelain(output);
        assert_eq!(result.len(), 2);
        assert!(result[0].is_main);
        assert!(!result[1].is_main);
        assert_eq!(result[1].branch, "feature");
    }

    #[test]
    fn parse_worktree_bare_skipped() {
        let output = "\
worktree /home/user/bare-repo
bare

worktree /home/user/repo-wt
HEAD aaa1111
branch refs/heads/dev

";
        let result = parse_worktree_porcelain(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "/home/user/repo-wt");
        assert!(result[0].is_main); // first non-bare entry
    }

    #[test]
    fn parse_worktree_no_trailing_newline() {
        let output = "worktree /home/user/repo\nHEAD abc1234\nbranch refs/heads/main";
        let result = parse_worktree_porcelain(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "/home/user/repo");
    }

    // --- parse_porcelain_status ---

    #[test]
    fn porcelain_status_modified_unstaged() {
        let output = " M src/main.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "src/main.rs");
        assert_eq!(result[0].status, "modified");
        assert!(!result[0].staged);
        assert!(result[0].unstaged);
    }

    #[test]
    fn porcelain_status_modified_staged() {
        let output = "M  src/main.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "modified");
        assert!(result[0].staged);
        assert!(!result[0].unstaged);
    }

    #[test]
    fn porcelain_status_modified_both() {
        let output = "MM src/main.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "modified");
        assert!(result[0].staged);
        assert!(result[0].unstaged);
    }

    #[test]
    fn porcelain_status_added() {
        let output = "A  new_file.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "added");
        assert!(result[0].staged);
    }

    #[test]
    fn porcelain_status_deleted() {
        let output = "D  old_file.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "deleted");
        assert!(result[0].staged);
    }

    #[test]
    fn porcelain_status_deleted_unstaged() {
        let output = " D old_file.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "deleted");
        assert!(!result[0].staged);
        assert!(result[0].unstaged);
    }

    #[test]
    fn porcelain_status_renamed() {
        let output = "R  old.rs -> new.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "renamed");
        assert_eq!(result[0].path, "old.rs -> new.rs");
    }

    #[test]
    fn porcelain_status_untracked() {
        let output = "?? untracked.txt\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "untracked");
        assert_eq!(result[0].path, "untracked.txt");
        assert!(!result[0].staged);
        assert!(!result[0].unstaged);
    }

    #[test]
    fn porcelain_status_multiple_files() {
        let output = " M file1.rs\nA  file2.rs\n?? file3.rs\nD  file4.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].status, "modified");
        assert_eq!(result[1].status, "added");
        assert_eq!(result[2].status, "untracked");
        assert_eq!(result[3].status, "deleted");
    }

    #[test]
    fn porcelain_status_empty_output() {
        let result = parse_porcelain_status("");
        assert!(result.is_empty());
    }

    #[test]
    fn porcelain_status_copied() {
        let output = "C  src.rs -> dst.rs\n";
        let result = parse_porcelain_status(output);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].status, "copied");
    }
}
