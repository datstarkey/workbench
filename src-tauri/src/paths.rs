use std::collections::HashSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn workbench_config_dir() -> PathBuf {
    home_dir().join(".workbench")
}

pub fn workbench_hook_socket_path() -> PathBuf {
    workbench_config_dir().join("claude-hooks.sock")
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
    fs::rename(&temp_path, path).context("Failed to rename temp file into place")?;
    Ok(())
}

/// Remove a file or directory if it exists. Handles symlinks correctly.
pub fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path)?;
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

/// Recursively copy a directory, following symlinks and detecting cycles.
/// Suitable for mirroring plugin/skill directories that may contain symlinks.
pub fn copy_dir_follow_symlinks(
    src: &Path,
    dst: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<()> {
    let canonical_src = fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    if !visited.insert(canonical_src.clone()) {
        return Ok(());
    }

    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let meta = fs::symlink_metadata(&src_path)?;

        if meta.file_type().is_symlink() {
            let resolved = match fs::canonicalize(&src_path) {
                Ok(path) => path,
                Err(_) => continue,
            };
            let resolved_meta = match fs::metadata(&resolved) {
                Ok(meta) => meta,
                Err(_) => continue,
            };

            if resolved_meta.is_dir() {
                copy_dir_follow_symlinks(&resolved, &dst_path, visited)?;
            } else if resolved_meta.is_file() {
                copy_file(&resolved, &dst_path)?;
            }
            continue;
        }

        if meta.is_dir() {
            copy_dir_follow_symlinks(&src_path, &dst_path, visited)?;
        } else if meta.is_file() {
            copy_file(&src_path, &dst_path)?;
        }
    }

    visited.remove(&canonical_src);
    Ok(())
}
