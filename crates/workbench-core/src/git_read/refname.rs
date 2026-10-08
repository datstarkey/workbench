//! git's `shorten_unambiguous_ref`, which `--abbrev-ref` and `%(refname:short)` print.

use std::collections::HashSet;

use anyhow::Result;
use gix::bstr::{BStr, BString, ByteSlice};
use gix::Repository;

const REV_PARSE_RULES: [(&str, &str); 6] = [
    ("", ""),
    ("refs/", ""),
    ("refs/tags/", ""),
    ("refs/heads/", ""),
    ("refs/remotes/", ""),
    ("refs/remotes/", "/HEAD"),
];

pub(super) struct RefNames<'r> {
    repo: &'r Repository,
    /// Every ref name, when many names will be shortened; otherwise each candidate is
    /// looked up on its own.
    all: Option<HashSet<BString>>,
    strict: bool,
}

impl<'r> RefNames<'r> {
    /// For shortening a name or two.
    pub(super) fn new(repo: &'r Repository) -> Self {
        Self {
            repo,
            all: None,
            strict: repo
                .config_snapshot()
                .boolean("core.warnAmbiguousRefs")
                .unwrap_or(true),
        }
    }

    /// For shortening a whole ref listing.
    pub(super) fn with_all_refs(repo: &'r Repository) -> Result<Self> {
        let mut all = HashSet::new();
        for reference in repo.references()?.all()? {
            let reference = reference.map_err(|e| anyhow::anyhow!("{e}"))?;
            all.insert(reference.name().as_bstr().to_owned());
        }
        Ok(Self {
            all: Some(all),
            ..Self::new(repo)
        })
    }

    fn exists(&self, name: &str) -> bool {
        if !name.starts_with("refs/") {
            return self.top_level_exists(name);
        }
        match &self.all {
            Some(all) => all.contains(BStr::new(name)),
            None => self
                .repo
                .try_find_reference(name)
                .ok()
                .flatten()
                .is_some_and(|r| r.name().as_bstr() == name),
        }
    }

    /// A top-level name (`HEAD`, `FETCH_HEAD`) is a ref only if its file in a git dir
    /// parses as one, so `.git/config` doesn't make a branch called `config` ambiguous.
    fn top_level_exists(&self, name: &str) -> bool {
        let cwd = self.repo.current_dir();
        [self.repo.git_dir(), self.repo.common_dir()]
            .iter()
            .filter_map(|dir| std::fs::read(cwd.join(dir).join(name)).ok())
            .any(|contents| {
                let line = contents.lines().next().unwrap_or_default().trim_end();
                line.starts_with(b"ref:")
                    || (matches!(line.len(), 40 | 64) && line.iter().all(u8::is_ascii_hexdigit))
            })
    }

    /// The shortest rule-derived name that no other rule resolves to an existing ref.
    pub(super) fn shorten(&self, full: &str) -> String {
        for i in (1..REV_PARSE_RULES.len()).rev() {
            let (prefix, suffix) = REV_PARSE_RULES[i];
            let Some(short) = full
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
                .filter(|short| !short.is_empty())
            else {
                continue;
            };
            let rules_to_fail = if self.strict {
                REV_PARSE_RULES.len()
            } else {
                i
            };
            let ambiguous = (0..rules_to_fail).filter(|&j| j != i).any(|j| {
                let (prefix, suffix) = REV_PARSE_RULES[j];
                self.exists(&format!("{prefix}{short}{suffix}"))
            });
            if !ambiguous {
                return short.to_string();
            }
        }
        full.to_string()
    }
}
