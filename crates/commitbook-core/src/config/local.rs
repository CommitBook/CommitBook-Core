use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Git-specific settings for the repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSettings {
    pub auto_push: bool,
    pub branch: String,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            auto_push: true,
            branch: "main".to_string(),
        }
    }
}

/// File pattern settings for selective commits.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileSettings {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Logging settings for the repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    pub level: String,
    pub max_log_files: u32,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            max_log_files: 30,
        }
    }
}

fn default_config_version() -> String {
    "1".to_string()
}

/// Local configuration stored at <repo>/.CommitBook/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalConfig {
    #[serde(default = "default_config_version")]
    pub config_version: String,
    pub enabled: bool,
    pub schedule: String,
    pub last_commit: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub git: GitSettings,
    #[serde(default)]
    pub files: FileSettings,
    #[serde(default)]
    pub logging: LoggingSettings,
    pub scheduler_id: Option<String>,
}

impl LocalConfig {
    /// Create a new default local config.
    pub fn new(schedule: &str) -> Self {
        Self {
            config_version: "1".to_string(),
            enabled: true,
            schedule: schedule.to_string(),
            last_commit: None,
            created_at: crate::utils::datetime::now_iso(),
            git: GitSettings::default(),
            files: FileSettings::default(),
            logging: LoggingSettings::default(),
            scheduler_id: None,
        }
    }

    /// Returns the .CommitBook directory path for a repo.
    pub fn commitbook_dir(repo_path: &Path) -> PathBuf {
        repo_path.join(".CommitBook")
    }

    /// Returns the config file path for a repo.
    pub fn config_path(repo_path: &Path) -> PathBuf {
        Self::commitbook_dir(repo_path).join("config.toml")
    }

    /// Returns the logs directory path for a repo.
    pub fn logs_dir(repo_path: &Path) -> PathBuf {
        Self::commitbook_dir(repo_path).join("logs")
    }

    /// Returns the lock file path for a repo.
    pub fn lock_path(repo_path: &Path) -> PathBuf {
        Self::commitbook_dir(repo_path).join(".lock")
    }

    /// Check if a repo has been set up with CommitBook.
    pub fn exists(repo_path: &Path) -> bool {
        Self::config_path(repo_path).exists()
    }

    /// Migrate config to the latest version. Returns true if migration occurred.
    pub fn migrate(&mut self) -> bool {
        // Currently at version "1" — no migrations needed yet.
        // Future versions will match on self.config_version and apply upgrades.
        false
    }

    /// Load local config from a repo.
    pub fn load(repo_path: &Path) -> Result<Self> {
        let path = Self::config_path(repo_path);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read local config: {}", path.display()))?;

        let mut config: Self = toml::from_str(&content)
            .with_context(|| "Failed to parse local config")?;

        if config.migrate() {
            config.save(repo_path)?;
        }

        Ok(config)
    }

    /// Save local config to the repo.
    pub fn save(&self, repo_path: &Path) -> Result<()> {
        let dir = Self::commitbook_dir(repo_path);
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create .CommitBook directory: {}", dir.display()))?;

        let path = Self::config_path(repo_path);
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize local config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write local config: {}", path.display()))?;

        Ok(())
    }

    /// Initialize the .CommitBook directory structure.
    pub fn init(repo_path: &Path, schedule: &str) -> Result<Self> {
        let cb_dir = Self::commitbook_dir(repo_path);
        let logs_dir = Self::logs_dir(repo_path);

        fs::create_dir_all(&cb_dir)
            .with_context(|| format!("Failed to create .CommitBook: {}", cb_dir.display()))?;
        fs::create_dir_all(&logs_dir)
            .with_context(|| format!("Failed to create logs dir: {}", logs_dir.display()))?;

        // Set directory permissions to 700 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&cb_dir, fs::Permissions::from_mode(0o700));
        }

        let config = Self::new(schedule);
        config.save(repo_path)?;

        // Add logs and lock to .gitignore
        Self::update_gitignore(repo_path)?;

        Ok(config)
    }

    /// Ensure .CommitBook/logs/ and .CommitBook/.lock are in .gitignore.
    fn update_gitignore(repo_path: &Path) -> Result<()> {
        let gitignore_path = repo_path.join(".gitignore");
        let entries = [".CommitBook/logs/", ".CommitBook/.lock"];

        let content = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path)
                .with_context(|| "Failed to read .gitignore")?
        } else {
            String::new()
        };

        let mut new_content = content.clone();
        let mut needs_update = false;

        for entry in &entries {
            if !new_content.lines().any(|line| line.trim() == *entry) {
                if !needs_update {
                    if !new_content.is_empty() && !new_content.ends_with('\n') {
                        new_content.push('\n');
                    }
                    new_content.push_str("\n# CommitBook\n");
                    needs_update = true;
                }
                new_content.push_str(&format!("{}\n", entry));
            }
        }

        if needs_update {
            fs::write(&gitignore_path, new_content)
                .with_context(|| "Failed to update .gitignore")?;
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
