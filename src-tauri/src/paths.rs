use std::fs;
use std::path::{Path, PathBuf};

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

/// Write content to a file atomically by writing to a temp file first,
/// then renaming into place. This prevents data corruption if the app
/// crashes mid-write.
pub fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let dir = path
        .parent()
        .context("Cannot determine parent directory")?;
    fs::create_dir_all(dir)?;

    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, content).context("Failed to write temp file")?;
    fs::rename(&temp_path, path).context("Failed to rename temp file into place")?;
    Ok(())
}
