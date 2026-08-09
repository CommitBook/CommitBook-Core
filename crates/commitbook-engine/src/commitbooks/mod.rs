//! Multi-CommitBook support layer.
//!
//! A "CommitBook" is a GitHub repo with `.CommitBook/` committed at its root.
//! This module models the *runtime* view of one, its identity, on-disk
//! location, and per-clone preferences, and provides registry operations
//! for scanning a `workspacesRoot` directory containing many such clones.
//!
//! No central registry file: subdirectories of `workspacesRoot` ARE the
//! registry. `scan_workspaces_root` reads each subdirectory's
//! `.CommitBook/config.toml` to materialize `CommitBook` summaries.

pub mod init;
pub mod preferences;
pub mod registry;
pub mod slug;

use std::path::PathBuf;

pub use init::init_dot_commitbook;
pub use preferences::{load_preferences, save_preferences, Preferences};
pub use registry::scan_workspaces_root;
pub use slug::slug_for;

/// Runtime view of a CommitBook clone on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitBook {
    /// `<owner>/<repo>`, stable across devices.
    pub id: String,
    pub owner: String,
    pub repo: String,
    pub name: String,
    pub provider: String,
    pub mode: String,
    pub branch: String,
    pub auto_sync: bool,
    /// `<workspacesRoot>/<owner>__<repo>` on disk.
    pub local_path: PathBuf,
}

impl CommitBook {
    pub fn id(owner: &str, repo: &str) -> String {
        format!("{owner}/{repo}")
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
