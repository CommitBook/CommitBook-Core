use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Optional token-backed authentication config stored in
/// `.CommitBook/local/auth.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    #[serde(default)]
    pub auth: AuthEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthEntry {
    pub provider: Option<String>,
    pub token: Option<String>,
}

impl AuthConfig {
    pub fn load(commitbook_dir: &Path) -> Result<Self> {
        let path = commitbook_dir.join("local").join("auth.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| "Failed to parse auth.toml")
    }

    pub fn save(&self, commitbook_dir: &Path) -> Result<()> {
        let local = commitbook_dir.join("local");
        std::fs::create_dir_all(&local)?;
        let path = local.join("auth.toml");
        let content =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize auth.toml")?;
        std::fs::write(&path, &content)
            .with_context(|| format!("Failed to write {}", path.display()))?;

        // Set restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    /// Delete `auth.toml`. Returns whether a file was removed.
    pub fn clear(commitbook_dir: &Path) -> Result<bool> {
        let path = commitbook_dir.join("local").join("auth.toml");
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).with_context(|| format!("Failed to remove {}", path.display())),
        }
    }

    pub fn has_token(&self) -> bool {
        self.auth.token.is_some()
    }

    pub fn token(&self) -> Option<&str> {
        self.auth.token.as_deref()
    }

    pub fn provider(&self) -> Option<&str> {
        self.auth.provider.as_deref()
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
