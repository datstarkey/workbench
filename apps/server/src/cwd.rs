//! The directory a terminal, chat or git query may run in.

use anyhow::{bail, Context, Result};

/// Resolve a request's working directory, restricted to directories Workbench
/// manages: a registered project, or a known worktree of one. This stops a
/// caller from running a shell or session in an arbitrary directory on the host.
pub fn resolve_cwd(project_path: &str, worktree_path: Option<&str>) -> Result<String> {
    let registered: Vec<String> = workbench_core::config::load_projects()?
        .into_iter()
        .map(|p| p.path)
        .collect();
    resolve_in(project_path, worktree_path, &registered)
}

fn resolve_in(
    project_path: &str,
    worktree_path: Option<&str>,
    registered_projects: &[String],
) -> Result<String> {
    // BOTH branches must resolve inside a registered Workbench project — a
    // worktree path is only trusted because its project is. Otherwise a caller
    // could run in any git repo's worktree on the host.
    if !is_registered_project(project_path, registered_projects) {
        bail!("project path is not a registered Workbench project: {project_path}");
    }
    match worktree_path {
        Some(wt) => {
            let worktrees = workbench_core::git::list_worktrees(project_path)
                .context("failed to list worktrees")?;
            if !worktrees.iter().any(|w| w.path == wt) {
                bail!("worktree path is not a known worktree of this project: {wt}");
            }
            Ok(wt.to_string())
        }
        None => {
            if !std::path::Path::new(project_path).is_dir() {
                bail!("project path does not exist: {project_path}");
            }
            Ok(project_path.to_string())
        }
    }
}

/// True if `path` is — or canonicalizes to — one of the registered project paths.
/// Canonicalizing tolerates a trailing slash, symlink, or `..` differing between
/// what the client sends and what's stored in `projects.json`.
fn is_registered_project(path: &str, registered: &[String]) -> bool {
    let canon = std::fs::canonicalize(path).ok();
    registered
        .iter()
        .any(|p| p == path || (canon.is_some() && std::fs::canonicalize(p).ok() == canon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_registered_project_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let registered = vec![path.to_string()];
        assert_eq!(resolve_in(path, None, &registered).unwrap(), path);
    }

    #[test]
    fn rejects_unregistered_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_in(dir.path().to_str().unwrap(), None, &[]).is_err());
    }

    #[test]
    fn rejects_missing_project_dir() {
        assert!(resolve_in("/no/such/dir", None, &[]).is_err());
    }

    #[test]
    fn rejects_worktree_in_unregistered_project() {
        // The worktree branch is gated by the same allowlist as the bare-project
        // branch: an unregistered project_path is rejected even with a worktree.
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_in(dir.path().to_str().unwrap(), Some("/tmp/wt"), &[]).is_err());
    }

    #[test]
    fn rejects_unknown_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let registered = vec![dir.path().to_str().unwrap().to_string()];
        // Registered project, but the worktree path isn't a known worktree (the
        // non-repo dir makes list_worktrees fail, which is also a rejection).
        let res = resolve_in(
            dir.path().to_str().unwrap(),
            Some("/tmp/elsewhere"),
            &registered,
        );
        assert!(res.is_err());
    }
}
