use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn workbench_config_dir() -> PathBuf {
    // Overridable so a headless server can point at an alternate config (and so
    // tests can isolate `~/.workbench` without touching the real home dir).
    if let Some(dir) = std::env::var_os("WORKBENCH_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    home_dir().join(".workbench")
}

pub fn claude_user_dir() -> PathBuf {
    home_dir().join(".claude")
}

pub fn codex_sessions_dir() -> PathBuf {
    home_dir().join(".codex").join("sessions")
}

pub fn codex_config_dir() -> PathBuf {
    home_dir().join(".codex")
}

/// Build a PATH that includes common CLI tool locations.
/// macOS/Linux GUI apps get a minimal PATH that excludes package manager bins,
/// so spawned commands like `gh` fail unless we enrich it.
/// The result is cached for the lifetime of the process since PATH doesn't change.
static ENRICHED_PATH: OnceLock<OsString> = OnceLock::new();

pub fn enriched_path() -> OsString {
    ENRICHED_PATH
        .get_or_init(|| {
            let mut dirs: Vec<PathBuf> = Vec::new();

            #[cfg(target_os = "macos")]
            {
                let home = home_dir();
                dirs.extend([
                    PathBuf::from("/opt/homebrew/bin"),
                    PathBuf::from("/usr/local/bin"),
                    home.join(".nix-profile/bin"),
                    PathBuf::from("/nix/var/nix/profiles/default/bin"),
                    PathBuf::from("/run/current-system/sw/bin"),
                ]);
            }

            #[cfg(target_os = "linux")]
            {
                let home = home_dir();
                dirs.extend([
                    PathBuf::from("/usr/local/bin"),
                    home.join(".nix-profile/bin"),
                    PathBuf::from("/nix/var/nix/profiles/default/bin"),
                    PathBuf::from("/run/current-system/sw/bin"),
                ]);
            }

            #[cfg(target_os = "windows")]
            {
                // Add common CLI tool install locations on Windows
                if let Ok(pf) = std::env::var("ProgramFiles") {
                    dirs.push(PathBuf::from(&pf).join("GitHub CLI"));
                }
                if let Ok(local) = std::env::var("LOCALAPPDATA") {
                    dirs.push(PathBuf::from(&local).join("Programs").join("GitHub CLI"));
                }
            }

            // Append the existing PATH so system defaults are still available
            if let Some(existing) = std::env::var_os("PATH") {
                for p in std::env::split_paths(&existing) {
                    if !dirs.contains(&p) {
                        dirs.push(p);
                    }
                }
            }

            #[cfg(not(windows))]
            let fallback = OsString::from("/usr/bin:/bin");
            #[cfg(windows)]
            let fallback = OsString::from("C:\\Windows\\System32;C:\\Windows");

            std::env::join_paths(dirs).unwrap_or(fallback)
        })
        .clone()
}

/// Encode a project path for use as a filename-safe identifier.
/// Replaces path separators and drive letter colons with `-`.
pub fn encode_project_path(project_path: &str) -> String {
    project_path
        .replace('\\', "-")
        .replace('/', "-")
        .replace(':', "")
}

/// Write a script file to disk, creating parent dirs and setting executable
/// permissions. Only writes if the content has changed.
pub fn ensure_script(path: &Path, body: &str) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let current = fs::read_to_string(path).unwrap_or_default();
    if current != body {
        fs::write(path, body)?;
    }
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(path.to_path_buf())
}

/// Write content to a file atomically by writing to a temp file first,
/// then renaming into place. This prevents data corruption if the app
/// crashes mid-write.
pub fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().context("Cannot determine parent directory")?;
    fs::create_dir_all(dir)?;

    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, content).context("Failed to write temp file")?;

    // On Windows, rename fails if the target exists and may be locked.
    // Remove the target first as a workaround.
    #[cfg(windows)]
    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&temp_path, path).context("Failed to rename temp file into place")?;
    Ok(())
}

/// Copy a file, creating parent directories as needed.
pub fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)?;
    Ok(())
}

/// Load a JSON file into a typed value, returning the provided default if the
/// file does not exist or cannot be parsed.
#[allow(dead_code)]
pub fn load_json<T: DeserializeOwned>(path: &Path, default: T) -> T {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return default,
        Err(e) => {
            log::warn!("[config] Failed to read {}: {e}", path.display());
            return default;
        }
    };
    match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[config] Failed to parse {}: {e}", path.display());
            default
        }
    }
}

/// Load a JSON file into a typed value, propagating errors.
/// Returns the provided default only when the file does not exist.
pub fn load_json_strict<T: DeserializeOwned>(path: &Path, default: T) -> Result<T> {
    if !path.exists() {
        return Ok(default);
    }
    let content = fs::read_to_string(path)?;
    let value: T = serde_json::from_str(&content)?;
    Ok(value)
}

/// Serialize a value to pretty JSON and write it atomically.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = serde_json::to_string_pretty(value)?;
    atomic_write(path, &content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // --- encode_project_path ---

    #[test]
    fn encode_project_path_typical() {
        assert_eq!(
            encode_project_path("/Users/jake/project"),
            "-Users-jake-project"
        );
    }

    #[test]
    fn encode_project_path_empty_string() {
        assert_eq!(encode_project_path(""), "");
    }

    #[test]
    fn encode_project_path_root() {
        assert_eq!(encode_project_path("/"), "-");
    }

    #[test]
    fn encode_project_path_no_slashes() {
        assert_eq!(encode_project_path("project"), "project");
    }

    #[test]
    fn encode_project_path_trailing_slash() {
        assert_eq!(
            encode_project_path("/Users/jake/project/"),
            "-Users-jake-project-"
        );
    }

    #[test]
    fn encode_project_path_windows_backslashes() {
        assert_eq!(
            encode_project_path("C:\\Users\\jake\\project"),
            "C-Users-jake-project"
        );
    }

    #[test]
    fn encode_project_path_windows_drive_letter() {
        assert_eq!(encode_project_path("D:\\repos\\my-app"), "D-repos-my-app");
    }

    #[test]
    fn encode_project_path_mixed_separators() {
        assert_eq!(
            encode_project_path("C:\\Users/jake\\project"),
            "C-Users-jake-project"
        );
    }

    // --- atomic_write ---

    #[test]
    fn test_atomic_write_and_read_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.json");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("file.txt");
        atomic_write(&path, "nested content").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "nested content");
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("overwrite.txt");
        atomic_write(&path, "first").unwrap();
        atomic_write(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    // --- copy_file ---

    #[test]
    fn test_copy_file_contents_match() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        fs::write(&src, "copy me").unwrap();
        copy_file(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "copy me");
    }

    #[test]
    fn test_copy_file_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("a").join("b").join("dst.txt");
        fs::write(&src, "deep copy").unwrap();
        copy_file(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "deep copy");
    }

    // --- ensure_script ---

    #[test]
    fn test_ensure_script_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("script.sh");
        let result = ensure_script(&path, "#!/bin/bash\necho hi").unwrap();
        assert_eq!(result, path);
        assert_eq!(fs::read_to_string(&path).unwrap(), "#!/bin/bash\necho hi");
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_script_sets_executable_permissions() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("exec.sh");
        ensure_script(&path, "#!/bin/bash").unwrap();
        let perms = fs::metadata(&path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o755);
    }

    #[test]
    fn test_ensure_script_no_rewrite_if_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("stable.sh");
        let body = "#!/bin/bash\necho stable";
        ensure_script(&path, body).unwrap();
        let mtime1 = fs::metadata(&path).unwrap().modified().unwrap();

        // Small sleep to ensure filesystem timestamp would differ if rewritten
        std::thread::sleep(std::time::Duration::from_millis(50));

        ensure_script(&path, body).unwrap();
        let mtime2 = fs::metadata(&path).unwrap().modified().unwrap();

        assert_eq!(mtime1, mtime2);
    }

    // --- enriched_path ---

    #[test]
    fn enriched_path_returns_nonempty() {
        let path = enriched_path();
        assert!(!path.is_empty());
    }

    #[test]
    fn enriched_path_contains_system_path() {
        // The enriched path should include entries from the system PATH
        let enriched = enriched_path();
        let enriched_str = enriched.to_string_lossy();
        // Should contain at least some path separator
        #[cfg(unix)]
        assert!(enriched_str.contains(':'));
        #[cfg(windows)]
        assert!(enriched_str.contains(';'));
    }

    // --- cross-platform atomic_write ---

    #[test]
    fn test_atomic_write_rapid_overwrites() {
        // Verify atomic_write handles rapid successive writes (tests the
        // Windows remove-before-rename path as well as the Unix atomic rename)
        let dir = tempdir().unwrap();
        let path = dir.path().join("rapid.json");
        for i in 0..10 {
            let content = format!("iteration {}", i);
            atomic_write(&path, &content).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
    }
}
