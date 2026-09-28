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

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AuthEntry {
    pub provider: Option<String>,
    pub token: Option<String>,
}

/// Never print the token, even in debug output or error chains.
impl std::fmt::Debug for AuthEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthEntry")
            .field("provider", &self.provider)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
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

    /// Write `auth.toml` atomically with mode 0600. A symlink or other
    /// non-regular file at that path is refused, never written through.
    pub fn save(&self, commitbook_dir: &Path) -> Result<()> {
        let local = commitbook_dir.join("local");
        std::fs::create_dir_all(&local)?;
        let path = local.join("auth.toml");
        let content =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize auth.toml")?;
        crate::config::local::write_private_text_atomic(&path, &content)
            .with_context(|| format!("Failed to write {}", path.display()))
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
