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
/// the `[commitbook]` block, are silently skipped. This means existing
/// non-FFI repos cloned into the same root won't pollute `list_commitbooks`,
/// and the absence of a registry file makes the filesystem itself the
/// source of truth.
pub fn scan_workspaces_root(workspaces_root: &Path) -> Result<Vec<CommitBook>> {
    if !workspaces_root.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(workspaces_root)
        .with_context(|| format!("Failed to read {}", workspaces_root.display()))?;

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let config_path = path.join(".CommitBook").join("config.toml");
        if !config_path.exists() {
            continue;
        }
        let config = match LocalConfig::load(&path) {
            Ok(c) => c,
            Err(_) => continue, // malformed config: skip, don't fail the whole scan
        };
        let Some(cb_settings) = config.commitbook else {
            continue;
        };

        let prefs = load_preferences(&path).unwrap_or_default();

        out.push(CommitBook {
            id: CommitBook::id(&cb_settings.owner, &cb_settings.repo),
            owner: cb_settings.owner,
            repo: cb_settings.repo,
            name: cb_settings.name,
            provider: cb_settings.provider,
            mode: cb_settings.mode,
            branch: config.git.branch,
            auto_sync: prefs.auto_sync,
            local_path: path,
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
