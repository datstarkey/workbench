//! Reading the saved model at boot, and saying when it can't be saved.

use std::path::{Path, PathBuf};

use serde::Serialize;
use workbench_core::workspace::persist::{self, WorkspacesFile};

/// Whether this process saves the model, shown to every client.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PersistStatus {
    #[default]
    Ok,
    /// Another process keeps the config dir's model.
    Locked,
    /// The saved model couldn't be read; nothing is saved over it.
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Persistence {
    pub status: PersistStatus,
    pub message: Option<String>,
}

impl Persistence {
    pub(super) fn locked(dir: &Path, pid: Option<u32>) -> Self {
        let who = pid.map_or("Another Workbench server".to_string(), |pid| {
            format!("Another Workbench server (pid {pid})")
        });
        Self {
            status: PersistStatus::Locked,
            message: Some(format!(
                "{who} is using {}: tabs opened here aren't saved.",
                dir.display()
            )),
        }
    }
}

/// The saved model, else the desktop's older `workspaces.json` migrated. On
/// any failure the file that failed is copied aside and the reason returned:
/// the caller must then never save, or it would write over the person's tabs.
pub(super) fn load(dir: &Path) -> Result<WorkspacesFile, Persistence> {
    persist::load(dir).map_err(|e| {
        let source = [persist::FILE, persist::LEGACY_FILE]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|path| path.exists());
        let kept = source.as_deref().and_then(keep_aside);
        tracing::error!(
            "workspace model not loaded from {}: {e:#}; kept at {}",
            source.as_deref().unwrap_or(dir).display(),
            kept.as_deref().map_or("nowhere".into(), |p| p.display().to_string())
        );
        let name = source
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| "the saved tabs".into(), |n| n.to_string_lossy().into_owned());
        let copy = kept
            .as_deref()
            .map_or(String::new(), |p| format!(" A copy is at {}.", p.display()));
        Persistence {
            status: PersistStatus::Error,
            message: Some(format!(
                "Couldn't read {name} ({e:#}), so tabs aren't saved until Workbench restarts with it fixed.{copy}"
            )),
        }
    })
}

/// Copy `path` to `<name>.broken-<unix seconds>` beside it.
fn keep_aside(path: &Path) -> Option<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = path.file_name()?.to_string_lossy();
    let copy = path.with_file_name(format!("{name}.broken-{secs}"));
    std::fs::copy(path, &copy).ok().map(|_| copy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_from_a_newer_version_is_kept_aside_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(persist::FILE),
            r#"{"version":99,"workspaces":[]}"#,
        )
        .unwrap();
        let err = load(dir.path()).unwrap_err();
        assert_eq!(err.status, PersistStatus::Error);
        assert!(err.message.unwrap().contains(persist::FILE));
        assert_eq!(broken(dir.path(), persist::FILE), 1);
    }

    #[test]
    fn a_corrupt_legacy_file_is_kept_aside_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(persist::LEGACY_FILE), "{ not json").unwrap();
        let err = load(dir.path()).unwrap_err();
        assert_eq!(err.status, PersistStatus::Error);
        assert_eq!(broken(dir.path(), persist::LEGACY_FILE), 1);
        assert!(!dir.path().join(persist::FILE).exists());
    }

    fn broken(dir: &Path, name: &str) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{name}.broken-"))
            })
            .count()
    }
}
