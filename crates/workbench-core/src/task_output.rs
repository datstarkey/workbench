//! Live output of Claude's background tasks (`run_in_background` shells,
//! background agents). The CLI writes each to
//! `<temp>/claude[-<uid>]/<encoded-cwd>/<run-id>/tasks/<task-id>.output`
//! — `%TEMP%\claude\…` on Windows, `/tmp/claude-<uid>/…` on macOS — and only
//! names the file when the task ends, so it's found by task id.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Task ids are short alphanumeric strings; anything else never touches the disk.
fn is_task_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn roots() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir()];
    if cfg!(unix) {
        dirs.push(PathBuf::from("/tmp"));
    }
    let mut roots = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if (name == "claude" || name.starts_with("claude-")) && entry.path().is_dir() {
                roots.push(entry.path());
            }
        }
    }
    roots
}

/// The output file of a background task, if it exists yet.
pub fn find(task_id: &str) -> Option<PathBuf> {
    find_in(&roots(), task_id)
}

fn find_in(roots: &[PathBuf], task_id: &str) -> Option<PathBuf> {
    if !is_task_id(task_id) {
        return None;
    }
    let file = format!("{task_id}.output");
    roots
        .iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flat_map(|projects| projects.flatten())
        .filter_map(|project| fs::read_dir(project.path()).ok())
        .flat_map(|runs| runs.flatten())
        .map(|run| run.path().join("tasks").join(&file))
        .find(|path| path.is_file())
}

/// The last `max_bytes` of the file (starting on a line boundary when cut)
/// and its total size.
pub fn tail(path: &Path, max_bytes: u64) -> std::io::Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        if let Some(nl) = text.find('\n') {
            text.drain(..=nl);
        }
    }
    Ok((text, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_task_by_id_and_tails_it() {
        let root = tempfile::tempdir().unwrap();
        let tasks = root
            .path()
            .join("-Users-me-repo")
            .join("run-1")
            .join("tasks");
        fs::create_dir_all(&tasks).unwrap();
        let out = tasks.join("bcsvseb10.output");
        fs::write(&out, "first line\nsecond line\nthird line\n").unwrap();

        let roots = vec![root.path().to_path_buf()];
        assert_eq!(find_in(&roots, "bcsvseb10"), Some(out.clone()));
        assert_eq!(find_in(&roots, "missing"), None);
        assert_eq!(find_in(&roots, "../../etc/passwd"), None);

        let (all, len) = tail(&out, 1024).unwrap();
        assert_eq!(all, "first line\nsecond line\nthird line\n");
        assert_eq!(len, all.len() as u64);
        let (end, _) = tail(&out, 15).unwrap();
        assert_eq!(end, "third line\n", "a cut tail starts on a whole line");
    }
}
