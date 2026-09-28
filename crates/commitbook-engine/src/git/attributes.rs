//! Detect `.gitattributes` filters that libgit2 cannot run.
//!
//! Git runs clean/smudge filter drivers such as git-crypt and Git LFS when
//! it stages a file. libgit2 only has its built-in line-ending and `ident`
//! handling: a `filter=<driver>` attribute is silently skipped, so sync would
//! commit and push the unfiltered file (plaintext instead of ciphertext, or a
//! large file instead of an LFS pointer). Sync, init, and doctor refuse such a
//! repository instead.

use anyhow::{Context, Result};
use std::fmt;
use std::path::Path;

/// One `filter=<driver>` attribute found in an attributes file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterUse {
    /// Attributes file, relative to the repository root (or as configured
    /// for `core.attributesFile`).
    pub source: String,
    pub pattern: String,
    pub filter: String,
}

impl fmt::Display for FilterUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`filter={}` for `{}` in {}",
            self.filter, self.pattern, self.source
        )
    }
}

/// Every `filter=<driver>` set by the attributes files that apply to the
/// repository: the root and nested `.gitattributes` (tracked or not),
/// `.git/info/attributes`, and `core.attributesFile`.
pub fn unsupported_filters(repo_root: &Path) -> Result<Vec<FilterUse>> {
    let repo = git2::Repository::open(repo_root).context("Failed to open repository")?;
    let mut found = Vec::new();

    let mut sources: Vec<(String, std::path::PathBuf)> = Vec::new();
    if let Some(workdir) = repo.workdir() {
        let is_attributes =
            |path: &str| path == ".gitattributes" || path.ends_with("/.gitattributes");
        let mut paths = std::collections::BTreeSet::new();
        // Tracked files come from the index; only untracked ones need a
        // worktree scan, which sync is about to stage.
        let index = repo.index().context("Failed to read the index")?;
        for entry in index.iter() {
            if let Ok(path) = std::str::from_utf8(&entry.path) {
                if is_attributes(path) {
                    paths.insert(path.to_string());
                }
            }
        }
        let mut options = git2::StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        let statuses = repo
            .statuses(Some(&mut options))
            .context("Failed to list repository files")?;
        for entry in statuses.iter() {
            if let Ok(path) = entry.path() {
                if is_attributes(path) {
                    paths.insert(path.to_string());
                }
            }
        }
        for path in paths {
            let absolute = workdir.join(&path);
            sources.push((path, absolute));
        }
    }
    sources.push((
        ".git/info/attributes".to_string(),
        repo.path().join("info").join("attributes"),
    ));
    if let Ok(configured) = repo
        .config()
        .and_then(|config| config.get_path("core.attributesFile"))
    {
        sources.push((configured.display().to_string(), configured));
    }

    for (source, path) in sources {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            // A file removed from the worktree, or not UTF-8: nothing to parse.
            Err(_) => continue,
        };
        found.extend(parse_filters(&source, &content));
    }
    Ok(found)
}

/// Parse `filter=<driver>` attributes from one attributes file.
pub(crate) fn parse_filters(source: &str, content: &str) -> Vec<FilterUse> {
    let mut found = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let Some(pattern) = tokens.next() else {
            continue;
        };
        // A macro definition (`[attr]name filter=...`) is reported too: it
        // exists to be applied to paths.
        for token in tokens {
            if let Some(filter) = token.strip_prefix("filter=") {
                if !filter.is_empty() {
                    found.push(FilterUse {
                        source: source.to_string(),
                        pattern: pattern.to_string(),
                        filter: filter.to_string(),
                    });
                }
            }
        }
    }
    found
}

/// Fail when the repository uses a filter libgit2 cannot run.
pub fn ensure_filters_supported(repo_root: &Path) -> Result<()> {
    let filters = unsupported_filters(repo_root)?;
    if filters.is_empty() {
        return Ok(());
    }
    let list = filters
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    anyhow::bail!(
        "This repository uses git filters CommitBook cannot run ({list}). Committing would store and push those files unfiltered (for example plaintext instead of git-crypt ciphertext, or whole files instead of Git LFS pointers). Remove the filter or keep this repository out of CommitBook."
    )
}

#[cfg(test)]
#[path = "attributes_tests.rs"]
mod tests;
