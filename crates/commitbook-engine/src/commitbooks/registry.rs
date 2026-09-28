use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::LocalConfig;
use crate::git::remote::{credential_free_url, get_remote_url, parse_remote_url};

use super::preferences::load_preferences;
use super::CommitBook;

/// A managed clone whose `.CommitBook/config.toml` could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenClone {
    pub path: PathBuf,
    pub error: String,
}

/// Result of `scan_workspaces`: the usable CommitBooks, plus the clones that
/// could not be loaded, which are reported instead of hiding the others.
#[derive(Debug, Default)]
pub struct WorkspaceScan {
    pub commitbooks: Vec<CommitBook>,
    pub broken: Vec<BrokenClone>,
}

/// The usable CommitBooks in `workspaces_root`; see `scan_workspaces`.
pub fn scan_workspaces_root(workspaces_root: &Path) -> Result<Vec<CommitBook>> {
    Ok(scan_workspaces(workspaces_root)?.commitbooks)
}

/// Scan `workspaces_root` for cloned CommitBooks. Each subdirectory that
/// contains `.CommitBook/config.toml` and a valid configured Git remote
/// becomes a `CommitBook` entry.
///
/// Subdirectories without `.CommitBook/`, or with a config that's missing
/// the `[commitbook]` block, are silently skipped. A real but invalid config
/// is reported in `broken` with clone context, so one clone with an old or
/// hand-broken config does not hide every other CommitBook. Scanning never
/// writes; persistence is reserved for mutation paths holding `RepoLock`.
pub fn scan_workspaces(workspaces_root: &Path) -> Result<WorkspaceScan> {
    if !workspaces_root.exists() {
        return Ok(WorkspaceScan::default());
    }

    let canonical_root = workspaces_root
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize {}", workspaces_root.display()))?;
    let entries = std::fs::read_dir(&canonical_root)
        .with_context(|| format!("Failed to read {}", workspaces_root.display()))?;

    let mut out = Vec::new();
    let mut broken = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // Never follow a workspace entry symlink. Otherwise merely listing
        // CommitBooks could read an arbitrary external `.CommitBook/config.toml`.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let Ok(canonical_path) = path.canonicalize() else {
            continue;
        };
        if canonical_path.parent() != Some(canonical_root.as_path()) {
            continue;
        }
        let commitbook_dir = canonical_path.join(".CommitBook");
        let config_path = commitbook_dir.join("config.toml");
        let safe_metadata = std::fs::symlink_metadata(&commitbook_dir)
            .ok()
            .filter(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
            .and_then(|_| std::fs::symlink_metadata(&config_path).ok())
            .is_some_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink());
        if !safe_metadata {
            continue;
        }
        // A real `.CommitBook/config.toml` identifies an intended managed
        // clone. Report a config that fails to load rather than skipping it
        // silently, which would look like "not found" at the API boundary.
        let config = match LocalConfig::load_read_only(&canonical_path) {
            Ok(config) => config,
            Err(error) => {
                broken.push(BrokenClone {
                    error: format!(
                        "Failed to load CommitBook config in {}: {error:#}",
                        canonical_path.display()
                    ),
                    path: canonical_path,
                });
                continue;
            }
        };
        let commitbook_local_id = match super::identity::load(&canonical_path) {
            Ok(id) => id,
            Err(error) => {
                broken.push(BrokenClone {
                    path: canonical_path,
                    error: format!("{error:#}"),
                });
                continue;
            }
        };
        // Remote metadata is descriptive, not the clone's identity.
        let Ok(remote_url) = get_remote_url(&canonical_path, &config.git.remote) else {
            continue;
        };
        let Ok(identity) = parse_remote_url(&remote_url) else {
            continue;
        };
        let Ok(remote_url) = credential_free_url(&remote_url) else {
            continue;
        };
        let mode = crate::devices::this_device(&canonical_path)
            .ok()
            .flatten()
            .map(|(_, device)| device.auth)
            .unwrap_or(crate::config::Auth::ExistingLocalRepo);

        let prefs = load_preferences(&canonical_path).unwrap_or_default();

        out.push(CommitBook {
            commitbook_local_id,
            remote_url,
            name: config.commitbook.name,
            provider: identity.provider.as_str().to_string(),
            mode: mode.as_str().to_string(),
            branch: config.git.branch,
            auto_sync: prefs.auto_sync,
            local_path: canonical_path,
        });
    }

    let mut counts = std::collections::HashMap::new();
    for cb in &out {
        *counts
            .entry(cb.commitbook_local_id.clone())
            .or_insert(0usize) += 1;
    }
    out.retain(|cb| {
        if counts[&cb.commitbook_local_id] > 1 {
            broken.push(BrokenClone { path: cb.local_path.clone(), error: format!(
                "Duplicate commitbook_local_id {} at {}; remove the intended copy's local/commitbook_local_id.toml and register it again",
                cb.commitbook_local_id, cb.local_path.display()) });
            false
        } else { true }
    });
    out.sort_by(|a, b| a.commitbook_local_id.cmp(&b.commitbook_local_id));
    broken.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(WorkspaceScan {
        commitbooks: out,
        broken,
    })
}

/// Find one local clone by persisted commitbook_local_id. Returns `None` if not
/// present in `workspaces_root`. When it is missing and some clones could not
/// be loaded, the error names them: the requested one may be among them.
pub fn find_by_id(workspaces_root: &Path, id: &str) -> Result<Option<CommitBook>> {
    let scan = scan_workspaces(workspaces_root)?;
    if let Some(found) = scan
        .commitbooks
        .into_iter()
        .find(|cb| cb.commitbook_local_id == id)
    {
        return Ok(Some(found));
    }
    if scan.broken.is_empty() {
        return Ok(None);
    }
    let details = scan
        .broken
        .iter()
        .map(|clone| clone.error.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    anyhow::bail!(
        "CommitBook {id} not found; {} clone(s) could not be loaded: {details}",
        scan.broken.len()
    )
}
