//! Parity: every gix read must equal its CLI twin on repositories built with the CLI.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::git;
use crate::git_read;

fn git_in(dir: &Path, date: Option<&str>, args: &[&str]) -> String {
    let mut cmd = crate::shell::tool("git");
    cmd.args([
        "-c",
        "user.name=Tëst Person",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "tag.gpgsign=false",
        "-c",
        "init.defaultBranch=main",
        "-c",
        "protocol.file.allow=always",
    ])
    .args(args)
    .current_dir(dir);
    if let Some(date) = date {
        cmd.env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, date: &str, message: &str) {
    git_in(
        dir,
        Some(date),
        &[
            "commit",
            "-q",
            "--allow-empty",
            "--cleanup=verbatim",
            "-m",
            message,
        ],
    );
}

/// Older git prints a UTC `%aI` as `+00:00`; git >= 2.45 (and gix_read) print `Z`.
fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value)
        .unwrap()
        .replace("+00:00\"", "Z\"")
}

/// Asserts gix == CLI where gix answers; returns whether it did.
fn check<T: Serialize>(what: &str, path: &str, fast: Result<Option<T>>, cli: Result<T>) -> bool {
    match (fast, cli) {
        (Ok(Some(fast)), Ok(cli)) => {
            assert_eq!(json(&fast), json(&cli), "{what} at {path}");
            true
        }
        (Ok(Some(fast)), Err(err)) => {
            panic!(
                "{what} at {path}: gix gave {} where git failed: {err}",
                json(&fast)
            )
        }
        (Ok(None), _) => false,
        (Err(err), _) => panic!("{what} at {path}: gix failed: {err:#}"),
    }
}

/// Checks every switched read at `path`; asserts gix handled the ones in `handled`.
fn check_all(path: &Path, handled: &[&str]) {
    let path = path.to_str().unwrap();
    let mut answered = Vec::new();
    let mut note = |name: &'static str, ok: bool| {
        if ok {
            answered.push(name);
        }
    };
    note(
        "git_info",
        check(
            "git_info",
            path,
            git_read::git_info(path),
            git::git_info_cli(path),
        ),
    );
    note(
        "list_worktrees",
        check(
            "list_worktrees",
            path,
            git_read::list_worktrees(path),
            git::list_worktrees_cli(path),
        ),
    );
    note(
        "list_branches",
        check(
            "list_branches",
            path,
            git_read::list_branches(path),
            git::list_branches_cli(path),
        ),
    );
    for max in [1, 3, 100] {
        note(
            "git_log",
            check(
                "git_log",
                path,
                git_read::git_log(path, max),
                git::git_log_cli(path, max),
            ),
        );
    }
    note(
        "git_stash_list",
        check(
            "git_stash_list",
            path,
            git_read::git_stash_list(path),
            git::git_stash_list_cli(path),
        ),
    );
    note(
        "status_branch",
        check(
            "status_branch",
            path,
            git_read::status_branch(path),
            Ok(git::status_branch_cli(path)),
        ),
    );
    for name in handled {
        assert!(answered.contains(name), "gix declined {name} at {path}");
    }
}

const ALL: &[&str] = &[
    "git_info",
    "list_worktrees",
    "list_branches",
    "git_log",
    "git_stash_list",
    "status_branch",
];

#[test]
fn parity_on_a_repo_with_remotes_worktrees_and_stashes() {
    let tmp = tempfile::tempdir().unwrap();
    // Not canonicalized: on macOS the temp dir is behind the /var -> /private/var symlink,
    // which git resolves in what it prints.
    let root = tmp.path();
    let repo = root.join("repo");
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&repo).unwrap();
    git_in(
        root,
        None,
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    git_in(&repo, None, &["init", "-q"]);

    commit(
        &repo,
        "2026-01-01T10:00:00+0000",
        "\n\n  base  \r\nsecond line\t\n\nbody",
    );
    git_in(
        &repo,
        None,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git_in(&repo, None, &["push", "-q", "-u", "origin", "main"]);
    git_in(&repo, None, &["remote", "set-head", "origin", "main"]);
    git_in(&repo, None, &["push", "-q", "origin", "main:remote-only"]);

    // Same-timestamp merges: git keeps ties in queue order.
    for b in ["a", "b", "c"] {
        git_in(&repo, None, &["checkout", "-q", "-b", b, "main"]);
        commit(&repo, "2026-01-02T10:00:00-0730", &format!("on {b}"));
    }
    git_in(&repo, None, &["checkout", "-q", "main"]);
    git_in(
        &repo,
        Some("2026-01-02T10:00:00+0530"),
        &["merge", "-q", "--no-ff", "-m", "octopus", "c", "a", "b"],
    );
    commit(
        &repo,
        "2026-01-02T10:00:00+0000",
        "same second as the merge",
    );
    commit(&repo, "2025-12-01T10:00:00+0100", "older than its parent");

    // Ambiguous short names: a tag and a branch called `amb`, a local `origin/main`.
    git_in(&repo, None, &["branch", "amb"]);
    git_in(&repo, None, &["tag", "amb"]);
    git_in(&repo, None, &["branch", "origin/main", "a"]);
    git_in(&repo, None, &["branch", "feature/nested", "b"]);
    git_in(&repo, None, &["branch", "config"]);
    git_in(&repo, None, &["branch", "index"]);

    std::fs::write(repo.join("file.txt"), "one").unwrap();
    git_in(&repo, None, &["add", "file.txt"]);
    commit(&repo, "2026-01-03T10:00:00+0000", "add file");
    std::fs::write(repo.join("file.txt"), "two").unwrap();
    git_in(
        &repo,
        Some("2026-01-04T10:00:00+0200"),
        &["stash", "push", "-q", "-m", "named stash"],
    );
    std::fs::write(repo.join("file.txt"), "three").unwrap();
    git_in(
        &repo,
        Some("2026-01-05T10:00:00-0500"),
        &["stash", "push", "-q"],
    );

    // Linked worktrees whose ids sort differently from their paths, plus mixed case.
    let wts = root.join("wts");
    for (dir, branch) in [("b/x", "wx"), ("a/y", "wy"), ("B/z", "wz")] {
        let p = wts.join(dir);
        git_in(
            &repo,
            None,
            &["worktree", "add", "-q", "-b", branch, p.to_str().unwrap()],
        );
    }
    let detached = wts.join("detached");
    git_in(
        &repo,
        None,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            detached.to_str().unwrap(),
        ],
    );
    let locked = wts.join("locked");
    git_in(
        &repo,
        None,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wl",
            locked.to_str().unwrap(),
        ],
    );
    git_in(&repo, None, &["worktree", "lock", locked.to_str().unwrap()]);
    let gone = wts.join("gone");
    git_in(
        &repo,
        None,
        &["worktree", "add", "-q", "-b", "wg", gone.to_str().unwrap()],
    );
    std::fs::remove_dir_all(&gone).unwrap();
    commit(
        &wts.join("a/y"),
        "2026-01-06T10:00:00+0000",
        "on a worktree",
    );

    check_all(&repo, ALL);
    check_all(&wts.join("b/x"), ALL);
    // A detached HEAD's branch row comes from its reflog; branches stay on the CLI there.
    check_all(
        &detached,
        &[
            "git_info",
            "list_worktrees",
            "git_log",
            "git_stash_list",
            "status_branch",
        ],
    );
    assert_eq!(
        json(&git::list_branches(detached.to_str().unwrap()).unwrap()),
        json(&git::list_branches_cli(detached.to_str().unwrap()).unwrap())
    );
    let branches = git::list_branches(repo.to_str().unwrap()).unwrap();
    assert!(
        !branches.iter().any(|b| b.name == "origin"),
        "origin/HEAD must not show as a branch"
    );

    // From a subdirectory the CLI compares an absolute --git-dir with a relative
    // --git-common-dir and calls the main checkout a worktree; gix reports it correctly.
    let sub = repo.join("subdir");
    std::fs::create_dir_all(&sub).unwrap();
    let sub = sub.to_str().unwrap();
    let fast = git_read::git_info(sub).unwrap().unwrap();
    let cli = git::git_info_cli(sub).unwrap();
    assert_eq!(
        (&fast.branch, &fast.repo_root),
        (&cli.branch, &cli.repo_root)
    );
    assert!(!fast.is_worktree);
}

#[test]
fn parity_on_a_submodule() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tmp.path().join("inner");
    let outer = tmp.path().join("outer");
    for dir in [&inner, &outer] {
        std::fs::create_dir_all(dir).unwrap();
        git_in(dir, None, &["init", "-q"]);
        commit(dir, "2026-02-01T10:00:00+0000", "first");
    }
    git_in(
        &outer,
        None,
        &["submodule", "add", "-q", inner.to_str().unwrap(), "sub"],
    );
    commit(&outer, "2026-02-02T10:00:00+0000", "add submodule");
    check_all(&outer, ALL);
    // A submodule's common dir is `.git/modules/sub`; its worktree listing stays on the CLI.
    check_all(
        &outer.join("sub"),
        &[
            "git_info",
            "list_branches",
            "git_log",
            "git_stash_list",
            "status_branch",
        ],
    );
}

#[test]
fn parity_on_an_unborn_repo() {
    let tmp = tempfile::tempdir().unwrap();
    git_in(tmp.path(), None, &["init", "-q"]);
    check_all(
        tmp.path(),
        &[
            "list_worktrees",
            "list_branches",
            "git_stash_list",
            "status_branch",
        ],
    );
    let path = tmp.path().to_str().unwrap();
    assert!(git::git_info(path).is_err());
    assert!(git::git_log(path, 10).is_err());
}

#[test]
fn parity_on_upstreams() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let repo = root.join("repo");
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&repo).unwrap();
    git_in(
        root,
        None,
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    git_in(&repo, None, &["init", "-q"]);
    git_in(
        &repo,
        None,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    commit(&repo, "2026-03-01T10:00:00+0000", "base");
    commit(&repo, "2026-03-02T10:00:00+0000", "pushed");
    git_in(&repo, None, &["push", "-q", "-u", "origin", "main"]);
    // Behind 1 (the pushed tip), ahead 2.
    git_in(&repo, None, &["reset", "-q", "--hard", "HEAD~1"]);
    commit(&repo, "2026-03-03T10:00:00+0000", "local one");
    commit(&repo, "2026-03-04T10:00:00+0000", "local two");
    let p = repo.to_str().unwrap();
    let main = git_read::status_branch(p).unwrap().unwrap();
    assert_eq!((main.ahead, main.behind, main.has_upstream), (2, 1, true));
    check_all(&repo, ALL);

    git_in(&repo, None, &["checkout", "-q", "-b", "gone"]);
    git_in(&repo, None, &["push", "-q", "-u", "origin", "gone"]);
    git_in(&repo, None, &["push", "-q", "origin", "--delete", "gone"]);
    check_all(&repo, ALL);
    assert!(!git_read::status_branch(p).unwrap().unwrap().has_upstream);

    git_in(&repo, None, &["checkout", "-q", "-b", "noup"]);
    check_all(&repo, ALL);

    // Named like a file in .git, which is not a ref.
    git_in(&repo, None, &["checkout", "-q", "-b", "config"]);
    check_all(&repo, ALL);
    assert_eq!(git_read::git_info(p).unwrap().unwrap().branch, "config");

    git_in(
        &repo,
        None,
        &["checkout", "-q", "-b", "localup", "--track", "main"],
    );
    commit(&repo, "2026-03-05T10:00:00+0000", "on localup");
    check_all(&repo, &["git_info", "list_branches", "git_log"]);
    let status = git::git_status(p).unwrap();
    let cli = git::status_branch_cli(p);
    assert_eq!(
        (
            status.branch,
            status.ahead,
            status.behind,
            status.has_upstream
        ),
        (cli.branch, cli.ahead, cli.behind, cli.has_upstream)
    );
}

#[test]
fn status_does_not_rewrite_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    git_in(dir, None, &["init", "-q"]);
    std::fs::write(dir.join("f.txt"), "same").unwrap();
    git_in(dir, None, &["add", "f.txt"]);
    commit(dir, "2026-04-01T10:00:00+0000", "add");
    let index = dir.join(".git/index");
    let before = std::fs::read(&index).unwrap();
    // Same content, new mtime: a refreshing `status` would rewrite the stat data.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(dir.join("f.txt"), "same").unwrap();
    let status = git::git_status(dir.to_str().unwrap()).unwrap();
    assert!(status.files.is_empty());
    assert_eq!(std::fs::read(&index).unwrap(), before);
}

#[test]
fn only_read_only_git_gets_the_short_timeout() {
    use git::{git_timeout, LONG_TIMEOUT, READ_TIMEOUT};
    assert_eq!(
        git_timeout(&["status", "--porcelain=v1", "-z"]),
        READ_TIMEOUT
    );
    assert_eq!(git_timeout(&["-c", "a=commit", "log"]), READ_TIMEOUT);
    assert_eq!(
        git_timeout(&["worktree", "list", "--porcelain"]),
        READ_TIMEOUT
    );
    assert_eq!(git_timeout(&["stash", "list"]), READ_TIMEOUT);
    assert_eq!(git_timeout(&["worktree", "add", "x"]), LONG_TIMEOUT);
    assert_eq!(git_timeout(&["stash", "push"]), LONG_TIMEOUT);
    assert_eq!(git_timeout(&["commit", "-m", "x"]), LONG_TIMEOUT);
    assert_eq!(git_timeout(&["push", "-u", "origin", "x"]), LONG_TIMEOUT);
    assert_eq!(git_timeout(&["clone", "u", "d"]), LONG_TIMEOUT);
}

#[test]
fn not_a_repo_still_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().to_str().unwrap();
    assert!(git::git_info(path).is_err());
    assert!(git::list_worktrees(path).is_err());
    assert!(git::list_branches(path).is_err());
}

#[test]
fn subject_matches_git_format_subject() {
    assert_eq!(
        git_read::subject(b"\n \nfirst  \r\nsecond\n\nbody"),
        "first second"
    );
    assert_eq!(git_read::subject(b"only"), "only");
    assert_eq!(git_read::subject(b""), "");
}
