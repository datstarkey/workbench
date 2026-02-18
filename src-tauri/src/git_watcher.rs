use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind, Debouncer};
use tauri::{AppHandle, Emitter, Manager};

use crate::refresh_dispatcher::RefreshDispatcher;
use crate::types::GitChangedEvent;

type FileWatcher = Debouncer<notify::RecommendedWatcher>;

pub struct GitWatcher {
    debouncer: Mutex<Option<FileWatcher>>,
    watched_paths: Mutex<HashSet<PathBuf>>,
}

impl GitWatcher {
    pub fn new(app_handle: AppHandle) -> Self {
        let debouncer = Self::create_debouncer(app_handle);

        Self {
            debouncer: Mutex::new(debouncer),
            watched_paths: Mutex::new(HashSet::new()),
        }
    }

    fn create_debouncer(app_handle: AppHandle) -> Option<FileWatcher> {
        let timeout = std::time::Duration::from_millis(500);
        let handle = app_handle.clone();

        let debouncer = new_debouncer(
            timeout,
            move |events: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                let events = match events {
                    Ok(e) => e,
                    Err(err) => {
                        eprintln!("[GitWatcher] watcher error: {err}");
                        return;
                    }
                };

                // Collect unique project paths from changed files
                let mut emitted = HashSet::new();
                for event in &events {
                    if event.kind != DebouncedEventKind::Any {
                        continue;
                    }
                    if let Some(project_path) = Self::project_path_from_git_path(&event.path) {
                        if emitted.insert(project_path.clone()) {
                            let project_path_string = project_path.to_string_lossy().to_string();
                            let dispatcher = handle.state::<RefreshDispatcher>();
                            dispatcher.request_refresh(
                                &handle,
                                project_path_string.clone(),
                                "git-watcher",
                                "git-dir-change",
                            );
                            let _ = handle.emit(
                                "git:changed",
                                GitChangedEvent {
                                    project_path: project_path_string,
                                },
                            );
                        }
                    }
                }
            },
        );

        match debouncer {
            Ok(d) => Some(d),
            Err(err) => {
                eprintln!("[GitWatcher] Failed to create debouncer: {err}");
                None
            }
        }
    }

    /// Given a path inside `.git/`, walk up to find the project root.
    fn project_path_from_git_path(path: &Path) -> Option<PathBuf> {
        let mut current = path;
        loop {
            if current.file_name().map(|f| f == ".git").unwrap_or(false) {
                return current.parent().map(|p| p.to_path_buf());
            }
            current = current.parent()?;
        }
    }

    /// Resolve the `.git` directory for a project path.
    /// For worktrees, `.git` is a file pointing to the main repo's `.git/worktrees/<name>`.
    /// We watch the main repo's `.git` in that case.
    fn resolve_git_dir(project_path: &Path) -> Option<PathBuf> {
        let dot_git = project_path.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        // Worktree: .git is a file containing "gitdir: <path>"
        if dot_git.is_file() {
            if let Ok(content) = std::fs::read_to_string(&dot_git) {
                if let Some(gitdir) = content.strip_prefix("gitdir: ") {
                    let gitdir = gitdir.trim();
                    let gitdir_path = if Path::new(gitdir).is_absolute() {
                        PathBuf::from(gitdir)
                    } else {
                        project_path.join(gitdir)
                    };
                    // Walk up from worktree gitdir to find the main .git dir
                    // e.g. /repo/.git/worktrees/foo -> /repo/.git
                    let mut p = gitdir_path.as_path();
                    while let Some(parent) = p.parent() {
                        if parent.file_name().map(|f| f == ".git").unwrap_or(false) {
                            return Some(parent.to_path_buf());
                        }
                        p = parent;
                    }
                }
            }
        }
        None
    }

    pub fn watch_project(&self, project_path: &str) -> Result<()> {
        let path = PathBuf::from(project_path);
        let git_dir = Self::resolve_git_dir(&path)
            .ok_or_else(|| anyhow!("No .git directory found for {project_path}"))?;

        let mut watched = self.watched_paths.lock().unwrap_or_else(|e| e.into_inner());
        if watched.contains(&path) {
            return Ok(());
        }

        let mut debouncer = self.debouncer.lock().unwrap_or_else(|e| e.into_inner());
        let watcher = debouncer
            .as_mut()
            .ok_or_else(|| anyhow!("File watcher not initialized"))?;

        // Watch .git/HEAD for branch switches
        let head = git_dir.join("HEAD");
        if head.exists() {
            watcher.watcher().watch(&head, RecursiveMode::NonRecursive)?;
        }

        // Watch .git/refs/ for new commits, branches, tags
        let refs = git_dir.join("refs");
        if refs.exists() {
            watcher.watcher().watch(&refs, RecursiveMode::Recursive)?;
        }

        watched.insert(path);
        Ok(())
    }

    pub fn sync_projects(&self, project_paths: Vec<String>) {
        let desired = normalize_project_paths(project_paths);
        let current = self
            .watched_paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let (to_watch, to_unwatch) = watch_diff(&current, &desired);

        for path in to_watch {
            let project_path = path.to_string_lossy().to_string();
            if let Err(err) = self.watch_project(&project_path) {
                eprintln!("[GitWatcher] Failed to watch {project_path}: {err}");
            }
        }

        for path in to_unwatch {
            let project_path = path.to_string_lossy().to_string();
            if let Err(err) = self.unwatch_project(&project_path) {
                eprintln!("[GitWatcher] Failed to unwatch {project_path}: {err}");
            }
        }
    }

    pub fn unwatch_project(&self, project_path: &str) -> Result<()> {
        let path = PathBuf::from(project_path);
        let git_dir = match Self::resolve_git_dir(&path) {
            Some(d) => d,
            None => return Ok(()), // nothing to unwatch
        };

        let mut watched = self.watched_paths.lock().unwrap_or_else(|e| e.into_inner());
        if !watched.remove(&path) {
            return Ok(());
        }

        let mut debouncer = self.debouncer.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(watcher) = debouncer.as_mut() {
            let head = git_dir.join("HEAD");
            if head.exists() {
                let _ = watcher.watcher().unwatch(&head);
            }
            let refs = git_dir.join("refs");
            if refs.exists() {
                let _ = watcher.watcher().unwatch(&refs);
            }
        }

        Ok(())
    }
}

fn normalize_project_paths(project_paths: Vec<String>) -> HashSet<PathBuf> {
    project_paths
        .into_iter()
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn watch_diff(current: &HashSet<PathBuf>, desired: &HashSet<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    (
        desired.difference(current).cloned().collect(),
        current.difference(desired).cloned().collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use super::{normalize_project_paths, watch_diff};

    #[test]
    fn normalize_project_paths_trims_dedupes_and_ignores_empty() {
        let set = normalize_project_paths(vec![
            "/repo/a".to_string(),
            " /repo/a ".to_string(),
            "".to_string(),
            "   ".to_string(),
            "/repo/b".to_string(),
        ]);

        assert_eq!(set.len(), 2);
        assert!(set.contains(&PathBuf::from("/repo/a")));
        assert!(set.contains(&PathBuf::from("/repo/b")));
    }

    #[test]
    fn watch_diff_returns_added_and_removed_paths() {
        let current = HashSet::from([PathBuf::from("/repo/a"), PathBuf::from("/repo/b")]);
        let desired = HashSet::from([PathBuf::from("/repo/b"), PathBuf::from("/repo/c")]);

        let (to_watch, to_unwatch) = watch_diff(&current, &desired);

        assert_eq!(
            to_watch.into_iter().collect::<HashSet<_>>(),
            HashSet::from([PathBuf::from("/repo/c")])
        );
        assert_eq!(
            to_unwatch.into_iter().collect::<HashSet<_>>(),
            HashSet::from([PathBuf::from("/repo/a")])
        );
    }

    #[test]
    fn watch_diff_noop_when_sets_match() {
        let current = HashSet::from([PathBuf::from("/repo/a")]);
        let desired = HashSet::from([PathBuf::from("/repo/a")]);

        let (to_watch, to_unwatch) = watch_diff(&current, &desired);

        assert!(to_watch.is_empty());
        assert!(to_unwatch.is_empty());
    }
}
