use anyhow::{Context, Result};
use std::path::Path;

use crate::config::LocalConfig;

use super::preferences::load_preferences;
use super::CommitBook;

/// Scan `workspaces_root` for cloned CommitBooks. Each subdirectory that
/// contains `.CommitBook/config.toml` with a populated `[commitbook]`
/// section becomes a `CommitBook` entry.
///
/// Subdirectories without `.CommitBook/`, or with a config that's missing
/// the `[commitbook]` block, are silently skipped. A real but invalid config
/// is surfaced with clone context. Scanning normalizes legacy fields only in
/// memory; persistence is reserved for mutation paths holding `RepoLock`.
pub fn scan_workspaces_root(workspaces_root: &Path) -> Result<Vec<CommitBook>> {
    if !workspaces_root.exists() {
        return Ok(Vec::new());
    }

    let canonical_root = workspaces_root
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize {}", workspaces_root.display()))?;
    let entries = std::fs::read_dir(&canonical_root)
        .with_context(|| format!("Failed to read {}", workspaces_root.display()))?;

    let mut out = Vec::new();
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
        // clone. Surface migration/configuration failures rather than turning
        // them into a misleading "not found" result at the API boundary.
        let config = LocalConfig::load_read_only(&canonical_path).map_err(|error| {
            anyhow::anyhow!(
                "Failed to load CommitBook config in {}: {error:#}",
                canonical_path.display()
            )
        })?;
        let Some(cb_settings) = config.commitbook else {
            continue;
        };

        let prefs = load_preferences(&canonical_path).unwrap_or_default();

        out.push(CommitBook {
            id: CommitBook::id(&cb_settings.owner, &cb_settings.repo),
            owner: cb_settings.owner,
            repo: cb_settings.repo,
            name: cb_settings.name,
            provider: cb_settings.provider,
            mode: cb_settings.mode,
            branch: config.git.branch,
            auto_sync: prefs.auto_sync,
            local_path: canonical_path,
        });
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Find a single CommitBook by id (`<owner>/<repo>`). Returns `None` if not
/// present in `workspaces_root`.
pub fn find_by_id(workspaces_root: &Path, id: &str) -> Result<Option<CommitBook>> {
    Ok(scan_workspaces_root(workspaces_root)?
        .into_iter()
        .find(|cb| cb.id == id))
}
