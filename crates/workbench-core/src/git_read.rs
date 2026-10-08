//! In-process (gix) versions of the read-only git queries the sidebar and watchers poll,
//! so a poll doesn't spawn a `git` per project. Each returns exactly what its CLI twin in
//! `git.rs` returns (see the parity tests), `Ok(None)` for a repository shape it leaves to
//! the CLI (unborn HEAD, ambiguous ref names, shallow history, ...), and `Err` when gix
//! fails; both fall back to the CLI through [`with_fallback`].

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use gix::bstr::{BStr, ByteSlice};
use gix::hash::ObjectId;
use gix::head::Kind as HeadKind;
use gix::repository::Kind as RepoKind;
use gix::Repository;

use crate::git::StatusBranch;
use crate::types::{BranchInfo, GitInfo, GitLogEntry, GitStashEntry, WorktreeInfo};
use refname::RefNames;

/// Run `fast`, or `cli` when it declines (`Ok(None)`) or fails. A failure is logged once per
/// `kind`, so a repository layout gix can't read costs one warning, never a broken sidebar.
pub(crate) fn with_fallback<T>(
    kind: &'static str,
    fast: impl FnOnce() -> Result<Option<T>>,
    cli: impl FnOnce() -> Result<T>,
) -> Result<T> {
    static WARNED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    match fast() {
        Ok(Some(value)) => return Ok(value),
        Ok(None) => {}
        Err(err) => {
            let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
            if !warned.contains(&kind) {
                warned.push(kind);
                log::warn!("gix {kind} failed, using the git CLI: {err:#}");
            }
        }
    }
    cli()
}

fn open(path: &str) -> Result<Repository> {
    let mut repo = gix::discover(path)?;
    // Each read opens afresh, so its first look at the packs is current. Without this,
    // every abbreviation lookup rescans the object directories (~1ms per ref here).
    repo.objects.refresh_never();
    Ok(repo)
}

/// A path as git prints it: real (symlinks resolved), and on Windows with forward slashes
/// and no verbatim prefix.
fn git_path(path: &Path) -> Result<String> {
    let real = std::fs::canonicalize(path)?;
    let s = real.to_str().context("non-UTF-8 path")?;
    #[cfg(windows)]
    {
        let s = match s.strip_prefix(r"\\?\UNC\") {
            Some(rest) => format!("//{rest}"),
            None => s.strip_prefix(r"\\?\").unwrap_or(s).to_string(),
        };
        Ok(s.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    Ok(s.to_string())
}

fn utf8(name: &BStr) -> Result<&str> {
    name.to_str().context("non-UTF-8 ref name")
}

pub(crate) fn git_info(path: &str) -> Result<Option<GitInfo>> {
    let repo = open(path)?;
    let Some(workdir) = repo.workdir() else {
        return Ok(None);
    };
    let branch = match repo.head()?.kind {
        HeadKind::Detached { .. } => "HEAD".to_string(),
        // `rev-parse HEAD` fails on an unborn branch; let the CLI report it.
        HeadKind::Unborn(_) => return Ok(None),
        HeadKind::Symbolic(r) => RefNames::new(&repo).shorten(utf8(r.name.as_bstr())?),
    };
    Ok(Some(GitInfo {
        branch,
        repo_root: git_path(workdir)?,
        is_worktree: repo.kind() == RepoKind::LinkedWorkTree,
    }))
}

/// `(head, branch)` as `git worktree list --porcelain` prints them.
fn worktree_head(repo: &Repository) -> Result<(String, String)> {
    let full = |name: &gix::refs::FullNameRef| -> Result<String> {
        let name = utf8(name.as_bstr())?;
        Ok(name.strip_prefix("refs/heads/").unwrap_or(name).to_string())
    };
    Ok(match repo.head()?.kind {
        HeadKind::Symbolic(r) => {
            let id = match r.target {
                gix::refs::Target::Object(id) => id,
                gix::refs::Target::Symbolic(_) => repo.head_id()?.detach(),
            };
            (id.to_string(), full(r.name.as_ref())?)
        }
        HeadKind::Unborn(name) => (
            ObjectId::null(repo.object_hash()).to_string(),
            full(name.as_ref())?,
        ),
        HeadKind::Detached { target, .. } => (target.to_string(), String::new()),
    })
}

pub(crate) fn list_worktrees(path: &str) -> Result<Option<Vec<WorktreeInfo>>> {
    let repo = open(path)?;
    let main = repo.main_repo()?;
    if main.is_bare() {
        return Ok(None);
    }
    let common = git_path(&repo.current_dir().join(repo.common_dir()))?;
    let Some(main_path) = common.strip_suffix("/.git") else {
        return Ok(None);
    };
    let (head, branch) = worktree_head(&main)?;
    let mut worktrees = vec![WorktreeInfo {
        path: main_path.to_string(),
        head,
        branch,
        is_main: true,
    }];

    let mut linked = Vec::new();
    for proxy in repo.worktrees()? {
        let gitdir = std::fs::read(proxy.git_dir().join("gitdir"))?;
        let gitdir = gitdir
            .trim_end()
            .to_str()
            .context("non-UTF-8 worktree path")?;
        // git resolves relative registrations through realpath; leave those to it.
        if gitdir.is_empty() || !Path::new(gitdir).is_absolute() {
            return Ok(None);
        }
        let wt_path = gitdir.strip_suffix("/.git").unwrap_or(gitdir).to_string();
        let (head, branch) =
            worktree_head(&proxy.into_repo_with_possibly_inaccessible_worktree()?)?;
        linked.push(WorktreeInfo {
            path: wt_path,
            head,
            branch,
            is_main: false,
        });
    }
    // `git worktree list` sorts linked worktrees by path (fspathcmp), main first.
    let ignore_case = repo
        .config_snapshot()
        .boolean("core.ignorecase")
        .unwrap_or(false);
    linked.sort_by(|a, b| path_cmp(&a.path, &b.path, ignore_case));
    worktrees.extend(linked);
    Ok(Some(worktrees))
}

fn path_cmp(a: &str, b: &str, ignore_case: bool) -> Ordering {
    if ignore_case {
        a.bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
    } else {
        a.cmp(b)
    }
}

pub(crate) fn list_branches(path: &str) -> Result<Option<Vec<BranchInfo>>> {
    let repo = open(path)?;
    // A detached HEAD adds a "(HEAD detached at …)" row whose wording comes from the
    // reflog and rebase/bisect state, and `branch.sort` reorders the list.
    if repo.config_snapshot().string("branch.sort").is_some() {
        return Ok(None);
    }
    let current = match repo.head()?.kind {
        HeadKind::Symbolic(r) => r.name,
        HeadKind::Unborn(name) => name,
        HeadKind::Detached { .. } => return Ok(None),
    };
    let names = RefNames::with_all_refs(&repo)?;
    let mut refs = Vec::new();
    for prefix in ["refs/heads/", "refs/remotes/"] {
        for reference in repo.references()?.prefixed(prefix)? {
            let Ok(mut reference) = reference else {
                continue;
            };
            // git skips broken refs (dangling symref or missing object).
            // Peeling moves a symbolic ref onto its target, so take the name first.
            let name = reference.name().as_bstr().to_owned();
            let Ok(id) = reference.peel_to_id() else {
                continue;
            };
            if !repo.has_object(id) {
                continue;
            }
            refs.push((name, id.shorten()?.to_string()));
        }
    }
    refs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut branches = Vec::new();
    for (full, sha) in refs {
        if full.ends_with(b"/HEAD") {
            continue;
        }
        let full_str = utf8(full.as_ref())?;
        branches.push(BranchInfo {
            name: names.shorten(full_str),
            sha,
            is_current: full == current.as_bstr(),
            is_remote: full_str.starts_with("refs/remotes/"),
        });
    }
    Ok(Some(branches))
}

/// `%aI`: strict ISO 8601 in the signature's own offset, `Z` for UTC (git >= 2.45).
fn iso_strict(time: gix::date::Time) -> Result<String> {
    let local = chrono::DateTime::from_timestamp(time.seconds + i64::from(time.offset), 0)
        .context("commit time out of range")?;
    let mut out = local.format("%Y-%m-%dT%H:%M:%S").to_string();
    if time.offset == 0 {
        out.push('Z');
    } else {
        let abs = time.offset.unsigned_abs() / 60;
        let sign = if time.offset < 0 { '-' } else { '+' };
        out.push_str(&format!("{sign}{:02}:{:02}", abs / 60, abs % 60));
    }
    Ok(out)
}

fn is_git_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// `%s`: the first paragraph after leading blank lines, its lines right-trimmed and joined
/// with spaces (git's `format_subject`).
fn subject(message: &[u8]) -> String {
    let mut out = Vec::new();
    for line in message.split_inclusive(|&b| b == b'\n') {
        let end = line
            .iter()
            .rposition(|&b| !is_git_space(b))
            .map_or(0, |i| i + 1);
        let line = &line[..end];
        if line.is_empty() {
            if out.is_empty() {
                continue;
            }
            break;
        }
        if !out.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(line);
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn commit_date(repo: &Repository, id: ObjectId) -> Result<i64> {
    Ok(repo.find_commit(id)?.committer()?.seconds())
}

/// The commit HEAD resolves to, or `None` for an unborn branch (`git log` fails there).
fn head_commit(repo: &Repository) -> Result<Option<ObjectId>> {
    if matches!(repo.head()?.kind, HeadKind::Unborn(_)) {
        return Ok(None);
    }
    Ok(Some(repo.head_id()?.detach()))
}

pub(crate) fn git_log(path: &str, max_count: u32) -> Result<Option<Vec<GitLogEntry>>> {
    let repo = open(path)?;
    // Grafted (shallow) and replaced history change what git walks.
    if repo.is_shallow()?
        || repo
            .references()?
            .prefixed("refs/replace/")?
            .next()
            .is_some()
    {
        return Ok(None);
    }
    let Some(head) = head_commit(&repo)? else {
        return Ok(None);
    };

    // git's default `log` order: newest committer date first, ties in the order the
    // commits were queued (gix's own queue doesn't keep ties stable).
    let mut queue = BinaryHeap::new();
    let mut seen = HashSet::from([head]);
    let mut queued = 0u64;
    queue.push((commit_date(&repo, head)?, Reverse(queued), head));
    let mut commits = Vec::new();
    while commits.len() < max_count as usize {
        let Some((_, _, id)) = queue.pop() else { break };
        let commit = repo.find_commit(id)?;
        for parent in commit.parent_ids() {
            let parent = parent.detach();
            if seen.insert(parent) {
                queued += 1;
                queue.push((commit_date(&repo, parent)?, Reverse(queued), parent));
            }
        }
        commits.push(commit);
    }

    let unpushed = unpushed_ids(&repo, head)?;
    let mut entries = Vec::with_capacity(commits.len());
    for commit in commits {
        let decoded = commit.decode()?;
        // git re-encodes legacy-encoded messages; leave those to it.
        if decoded
            .encoding
            .is_some_and(|e| !e.eq_ignore_ascii_case(b"utf-8") && !e.eq_ignore_ascii_case(b"utf8"))
        {
            return Ok(None);
        }
        let author = commit.author()?;
        let sha = commit.id().to_string();
        entries.push(GitLogEntry {
            unpushed: unpushed.contains(&commit.id),
            short_sha: commit.id().shorten()?.to_string(),
            message: subject(decoded.message),
            author: String::from_utf8_lossy(author.name.trim_end()).into_owned(),
            date: iso_strict(author.time()?)?,
            sha,
        });
    }
    Ok(Some(entries))
}

/// Commits reachable from HEAD but from no remote-tracking ref (`rev-list HEAD --not
/// --remotes --max-count=1000`), empty without remotes.
fn unpushed_ids(repo: &Repository, head: ObjectId) -> Result<HashSet<ObjectId>> {
    if repo.remote_names().is_empty() {
        return Ok(HashSet::new());
    }
    let mut hidden = Vec::new();
    for reference in repo.references()?.prefixed("refs/remotes/")? {
        let Ok(mut reference) = reference else {
            continue;
        };
        if let Ok(id) = reference.peel_to_id() {
            hidden.push(id.detach());
        }
    }
    let mut ids = HashSet::new();
    // Date order, as `rev-list` picks its 1000.
    let walk = repo.rev_walk([head]).with_hidden(hidden).sorting(
        gix::revision::walk::Sorting::ByCommitTime(Default::default()),
    );
    for info in walk.all()?.take(1000) {
        ids.insert(info?.id);
    }
    Ok(ids)
}

/// The branch half of `git status`: `rev-parse --abbrev-ref HEAD` (`HEAD` when it fails)
/// and `rev-list --left-right --count @{u}...HEAD`.
pub(crate) fn status_branch(path: &str) -> Result<Option<StatusBranch>> {
    let repo = open(path)?;
    let no_upstream = |branch: String| StatusBranch {
        branch,
        ahead: 0,
        behind: 0,
        has_upstream: false,
    };
    let (name, head) = match repo.head()?.kind {
        HeadKind::Symbolic(r) => (r.name, repo.head_id()?.detach()),
        HeadKind::Unborn(_) | HeadKind::Detached { .. } => {
            return Ok(Some(no_upstream("HEAD".to_string())))
        }
    };
    let branch = RefNames::new(&repo).shorten(utf8(name.as_bstr())?);
    let short = utf8(name.as_bstr())?
        .strip_prefix("refs/heads/")
        .unwrap_or_default();
    // An upstream that is another local branch (`branch.<b>.remote = .`) stays on the CLI.
    if repo
        .config_snapshot()
        .string(format!("branch.{short}.remote").as_str())
        .is_some_and(|remote| remote.as_slice() == b".")
    {
        return Ok(None);
    }
    let Some(tracking) =
        repo.branch_remote_tracking_ref_name(name.as_ref(), gix::remote::Direction::Fetch)
    else {
        return Ok(Some(no_upstream(branch)));
    };
    // A configured upstream whose tracking ref is gone fails `@{u}` like no upstream.
    let Some(mut upstream) = repo.try_find_reference(tracking?.as_ref())? else {
        return Ok(Some(no_upstream(branch)));
    };
    let upstream = upstream.peel_to_id()?.detach();
    let count = |tip: ObjectId, hidden: ObjectId| -> Result<u32> {
        let mut n = 0;
        for info in repo.rev_walk([tip]).with_hidden([hidden]).all()? {
            info?;
            n += 1;
        }
        Ok(n)
    };
    Ok(Some(StatusBranch {
        branch,
        ahead: count(head, upstream)?,
        behind: count(upstream, head)?,
        has_upstream: true,
    }))
}

pub(crate) fn git_stash_list(path: &str) -> Result<Option<Vec<GitStashEntry>>> {
    let repo = open(path)?;
    let Some(stash) = repo.try_find_reference("refs/stash")? else {
        return Ok(Some(Vec::new()));
    };
    let mut log = stash.log_iter();
    // Without a reflog git lists the stash ref itself; leave that to it.
    let Some(lines) = log.all()? else {
        return Ok(None);
    };
    let mut stashes = Vec::new();
    for line in lines {
        let line = line?;
        stashes.push((ObjectId::from_hex(line.new_oid)?, line.message.to_owned()));
    }
    let mut entries = Vec::with_capacity(stashes.len());
    for (index, (id, message)) in stashes.into_iter().rev().enumerate() {
        let commit = repo.find_commit(id)?;
        entries.push(GitStashEntry {
            index: index as u32,
            message: String::from_utf8_lossy(&message).into_owned(),
            date: iso_strict(commit.author()?.time()?)?,
        });
    }
    Ok(Some(entries))
}

#[cfg(test)]
mod bench;
mod refname;
#[cfg(test)]
mod tests;
