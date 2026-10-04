//! The files in a checkout, for the chat composer's `@` mentions.

use anyhow::Result;

/// Enough for most repos; a bigger one is cut off rather than sent whole.
pub const MAX_LISTED: usize = 10_000;

/// Tracked and untracked files under `cwd` (relative paths), minus anything
/// `.gitignore` excludes, sorted, at most `limit`.
pub fn list(cwd: &str, limit: usize) -> Result<Vec<String>> {
    let out = crate::git::git_output(
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ],
        cwd,
    )?;
    let mut files: Vec<String> = out
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .collect();
    files.sort_unstable();
    files.dedup();
    files.truncate(limit);
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_tracked_and_untracked_files_but_not_ignored_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        crate::git::git_output(&["init", "-q"], root).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "").unwrap();
        std::fs::write(dir.path().join("README.md"), "").unwrap();
        std::fs::write(dir.path().join("debug.log"), "").unwrap();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/out"), "").unwrap();
        crate::git::git_output(&["add", "README.md"], root).unwrap();

        assert_eq!(
            list(root, MAX_LISTED).unwrap(),
            [".gitignore", "README.md", "src/main.rs"]
        );
        assert_eq!(list(root, 1).unwrap(), [".gitignore"]);
    }

    #[test]
    fn fails_outside_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list(dir.path().to_str().unwrap(), MAX_LISTED).is_err());
    }
}
