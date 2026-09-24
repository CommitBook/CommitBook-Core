use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingInitPush {
    pub commit_oid: String,
    pub remote: String,
    pub branch: String,
}

/// Sync state stored in `.CommitBook/local/state.toml`.
///
/// Persistent across runs. `remote_head` from older versions of CommitBook
/// is no longer needed (derivable via `git rev-parse origin/<branch>`); it
/// loads silently from old `state.toml` files via serde's default
/// unknown-field tolerance.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    #[serde(default)]
    pub last_attempt_at: Option<String>,
    #[serde(default)]
    pub last_fetch_at: Option<String>,
    #[serde(default)]
    pub last_push_at: Option<String>,
    #[serde(default)]
    pub last_error_stage: Option<String>,
    #[serde(default)]
    pub last_sync_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    /// Notes where the most recent `both`-mode merge kept two versions.
    /// Replaced by the next cycle that keeps both; cleared by nothing else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kept_both_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_both_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_init_push: Option<PendingInitPush>,
}

impl SyncState {
    pub fn load(commitbook_dir: &Path) -> Result<Self> {
        let path = commitbook_dir.join("local").join("state.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| "Failed to parse state.toml")
    }

    pub fn save(&self, commitbook_dir: &Path) -> Result<()> {
        let local = commitbook_dir.join("local");
        std::fs::create_dir_all(&local)?;
        let path = local.join("state.toml");
        let content =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize state.toml")?;
        use std::io::Write;
        let mut temporary = tempfile::NamedTempFile::new_in(&local)?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&path)
            .with_context(|| format!("Failed to atomically write {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "sync_state_tests.rs"]
mod tests;
