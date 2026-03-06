use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn workbench_config_dir() -> PathBuf {
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

pub fn agents_dir() -> PathBuf {
    home_dir().join(".agents")
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

/// Remove a file, directory, or symlink if it exists.
/// Uses `symlink_metadata` so broken symlinks are detected and removed.
pub fn remove_path_if_exists(path: &Path) -> Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if meta.is_dir() && !meta.file_type().is_symlink() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
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

/// Recursively copy a directory, skipping symlinks.
/// Suitable for copying workspace files (env, config) to worktrees.
pub fn copy_dir_skip_symlinks(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let source_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        let meta = fs::symlink_metadata(&source_path)?;

        if meta.file_type().is_symlink() {
            continue;
        } else if meta.is_dir() {
            copy_dir_skip_symlinks(&source_path, &dest_path)?;
        } else if meta.is_file() {
            copy_file(&source_path, &dest_path)?;
        }
    }
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
        assert_eq!(
            encode_project_path("D:\\repos\\my-app"),
            "D-repos-my-app"
        );
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

    // --- remove_path_if_exists ---

    #[test]
    fn test_remove_path_if_exists_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("removeme.txt");
        fs::write(&path, "bye").unwrap();
        assert!(path.exists());
        remove_path_if_exists(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_remove_path_if_exists_directory() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("subdir");
        fs::create_dir_all(sub.join("inner")).unwrap();
        fs::write(sub.join("inner").join("file.txt"), "data").unwrap();
        assert!(sub.exists());
        remove_path_if_exists(&sub).unwrap();
        assert!(!sub.exists());
    }

    #[test]
    fn test_remove_path_if_exists_nonexistent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ghost.txt");
        assert!(remove_path_if_exists(&path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_path_if_exists_broken_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let link = dir.path().join("broken_link");
        symlink(dir.path().join("nonexistent"), &link).unwrap();
        assert!(fs::symlink_metadata(&link).is_ok()); // link entry exists
        assert!(!link.exists()); // but target does not

        remove_path_if_exists(&link).unwrap();
        assert!(!fs::symlink_metadata(&link).is_ok()); // link removed
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

    // --- copy_dir_skip_symlinks ---

    #[test]
    fn test_copy_dir_skip_symlinks_copies_files() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("root.txt"), "root").unwrap();
        fs::write(src.join("sub").join("nested.txt"), "nested").unwrap();

        copy_dir_skip_symlinks(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("root.txt")).unwrap(), "root");
        assert_eq!(
            fs::read_to_string(dst.join("sub").join("nested.txt")).unwrap(),
            "nested"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_dir_skip_symlinks_skips_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("real.txt"), "real").unwrap();
        symlink(src.join("real.txt"), src.join("link.txt")).unwrap();

        copy_dir_skip_symlinks(&src, &dst).unwrap();

        assert!(dst.join("real.txt").exists());
        assert!(!dst.join("link.txt").exists());
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
