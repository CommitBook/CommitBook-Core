use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Sync state stored in `.CommitBook/state.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    pub remote_head: Option<String>,
    pub last_sync_at: Option<String>,
}

impl SyncState {
    pub fn load(commitbook_dir: &Path) -> Result<Self> {
        let path = commitbook_dir.join("state.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| "Failed to parse state.toml")
    }

    pub fn save(&self, commitbook_dir: &Path) -> Result<()> {
        let path = commitbook_dir.join("state.toml");
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize state.toml")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }
}
