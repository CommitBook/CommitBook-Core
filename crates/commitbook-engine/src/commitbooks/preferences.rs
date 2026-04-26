use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Per-clone, per-device preferences. Lives at
/// `<clone>/.CommitBook/local/preferences.toml` (gitignored).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default = "default_auto_sync")]
    pub auto_sync: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { auto_sync: true }
    }
}

fn default_auto_sync() -> bool {
    true
}

/// Load preferences for a clone. Returns defaults if the file doesn't exist
/// (treated as "not yet customized").
pub fn load_preferences(repo_root: &Path) -> Result<Preferences> {
    let path = repo_root
        .join(".CommitBook")
        .join("local")
        .join("preferences.toml");
    if !path.exists() {
        return Ok(Preferences::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))
}

/// Save preferences for a clone. Creates the parent directory if missing.
pub fn save_preferences(repo_root: &Path, prefs: &Preferences) -> Result<()> {
    let local = repo_root.join(".CommitBook").join("local");
    std::fs::create_dir_all(&local)
        .with_context(|| format!("Failed to create {}", local.display()))?;
    let path = local.join("preferences.toml");
    let content = toml::to_string_pretty(prefs).context("Failed to serialize preferences")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
