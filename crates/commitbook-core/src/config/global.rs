use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Entry for a registered repository in the global config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub enabled: bool,
    pub schedule: String,
}

/// AI provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Priority order for commit message generation.
    pub providers: Vec<String>,
    /// Path to the gh CLI binary.
    pub gh_copilot_path: Option<String>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            providers: vec![
                "gh-copilot".to_string(),
                "claude-cli".to_string(),
                "codex-cli".to_string(),
                "fallback".to_string(),
            ],
            gh_copilot_path: None,
        }
    }
}

/// Global configuration stored at ~/.CommitBook/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    pub version: String,
    #[serde(default)]
    pub repos: HashMap<String, RepoEntry>,
    #[serde(default)]
    pub ai: AiConfig,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            repos: HashMap::new(),
            ai: AiConfig::default(),
        }
    }
}

impl GlobalConfig {
    /// Returns the path to the global config directory.
    pub fn config_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".CommitBook"))
    }

    /// Returns the path to the global config file.
    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    /// Migrate config to the latest version. Returns true if migration occurred.
    pub fn migrate(&mut self) -> bool {
        // Currently at version "1.0.0" — no migrations needed yet.
        false
    }

    /// Load global config from disk. Creates default if not found.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read global config: {}", path.display()))?;

        let mut config: Self = toml::from_str(&content)
            .with_context(|| "Failed to parse global config")?;

        if config.migrate() {
            config.save()?;
        }

        Ok(config)
    }

    /// Save global config to disk.
    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;

        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize global config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write global config: {}", path.display()))?;

        Ok(())
    }

    /// Register a repo in the global config.
    pub fn register_repo(&mut self, repo_path: &str, schedule: &str) -> Result<()> {
        self.repos.insert(
            repo_path.to_string(),
            RepoEntry {
                enabled: true,
                schedule: schedule.to_string(),
            },
        );
        self.save()
    }

    /// Update the enabled state for a repo.
    pub fn set_repo_enabled(&mut self, repo_path: &str, enabled: bool) -> Result<()> {
        if let Some(entry) = self.repos.get_mut(repo_path) {
            entry.enabled = enabled;
            self.save()?;
        }
        Ok(())
    }

    /// Update the schedule for a repo.
    pub fn set_repo_schedule(&mut self, repo_path: &str, schedule: &str) -> Result<()> {
        if let Some(entry) = self.repos.get_mut(repo_path) {
            entry.schedule = schedule.to_string();
            self.save()?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "global_tests.rs"]
mod tests;
