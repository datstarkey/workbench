//! The directory a terminal, chat, git query or worktree change may run in.

use anyhow::{bail, Context, Result};

/// A path outside the directories Workbench manages. The server answers it
/// with 403, so a caller can tell a refusal from a failure.
#[derive(Debug)]
pub struct NotAllowed(pub String);

impl std::fmt::Display for NotAllowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotAllowed {}

/// Resolve a request's working directory, restricted to directories Workbench
/// manages: a registered project, or a known worktree of one. This stops a
/// caller from running a shell or session in an arbitrary directory on the host.
pub fn resolve_cwd(project_path: &str, worktree_path: Option<&str>) -> Result<String> {
    resolve_in(project_path, worktree_path, &registered()?)
}

pub(crate) fn registered() -> Result<Vec<String>> {
    Ok(crate::config::load_projects()?
        .into_iter()
        .map(|p| p.path)
        .collect())
}

pub(crate) fn resolve_in(
    project_path: &str,
    worktree_path: Option<&str>,
    registered_projects: &[String],
) -> Result<String> {
    // BOTH branches must resolve inside a registered Workbench project — a
    // worktree path is only trusted because its project is. Otherwise a caller
    // could run in any git repo's worktree on the host.
    if !is_registered_project(project_path, registered_projects) {
        return Err(NotAllowed(format!(
            "project path is not a registered Workbench project: {project_path}"
        ))
        .into());
    }
    match worktree_path {
        Some(wt) => {
            let worktrees =
                crate::git::list_worktrees(project_path).context("failed to list worktrees")?;
            if !worktrees.iter().any(|w| w.path == wt) {
                return Err(NotAllowed(format!(
                    "worktree path is not a known worktree of this project: {wt}"
                ))
                .into());
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
        let err = resolve_in(dir.path().to_str().unwrap(), None, &[]).unwrap_err();
        assert!(err.downcast_ref::<NotAllowed>().is_some(), "{err}");
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
        // A real repo, so the known-worktree guard runs rather than list_worktrees failing.
        let ok = crate::shell::command("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let registered = vec![dir.path().to_str().unwrap().to_string()];
        let err = resolve_in(
            dir.path().to_str().unwrap(),
            Some("/tmp/elsewhere"),
            &registered,
        )
        .unwrap_err();
        assert!(err.downcast_ref::<NotAllowed>().is_some(), "{err}");
        assert!(err.to_string().contains("not a known worktree"), "{err}");
    }
}
