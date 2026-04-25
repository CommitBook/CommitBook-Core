use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::state::auth::{AuthConfig, AuthEntry};

/// Provider + token pair managed by a `SecretStore`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretBundle {
    pub provider: Option<String>,
    pub token: Option<String>,
}

/// Credential storage abstraction.
///
/// Desktop uses `FileSecretStore`, which reads and writes
/// `.CommitBook/local/auth.toml` with `0o600` permissions. Mobile hosts
/// implement this trait against iOS Keychain / Android Keystore and pass it
/// across the FFI boundary.
pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<SecretBundle>;
    fn save(&self, bundle: &SecretBundle) -> Result<()>;
}

/// File-backed `SecretStore` that delegates to `AuthConfig`.
pub struct FileSecretStore {
    commitbook_dir: PathBuf,
}

impl FileSecretStore {
    pub fn new(commitbook_dir: impl AsRef<Path>) -> Self {
        Self {
            commitbook_dir: commitbook_dir.as_ref().to_path_buf(),
        }
    }
}

impl SecretStore for FileSecretStore {
    fn load(&self) -> Result<SecretBundle> {
        let config = AuthConfig::load(&self.commitbook_dir)?;
        Ok(SecretBundle {
            provider: config.auth.provider,
            token: config.auth.token,
        })
    }

    fn save(&self, bundle: &SecretBundle) -> Result<()> {
        let config = AuthConfig {
            auth: AuthEntry {
                provider: bundle.provider.clone(),
                token: bundle.token.clone(),
            },
        };
        config.save(&self.commitbook_dir)
    }
}
