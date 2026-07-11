use anyhow::Result;
use git2::{Config, Cred, CredentialType};

/// Supplies credentials to libgit2 during authenticated network operations.
///
/// Desktop uses `SystemCredentials` (delegates to git's credential-helper stack:
/// OSXKeychain, `.git-credentials`, `~/.netrc`, etc.). Mobile uses
/// `TokenCredentials` with a PAT fetched via the `SecretStore` trait.
pub trait CredentialProvider: Send + Sync {
    fn provide(
        &self,
        url: &str,
        username_from_url: Option<&str>,
        allowed: CredentialType,
    ) -> Result<Cred>;
}

/// Credential provider that delegates to git's standard credential-helper
/// stack. Preserves existing desktop UX, tokens stored in OSXKeychain /
/// Windows Credential Manager / `.git-credentials` all work unchanged.
pub struct SystemCredentials;

impl CredentialProvider for SystemCredentials {
    fn provide(
        &self,
        url: &str,
        username_from_url: Option<&str>,
        allowed: CredentialType,
    ) -> Result<Cred> {
        let config = Config::open_default()?;
        let cred = Cred::credential_helper(&config, url, username_from_url)
            .or_else(|_| {
                if allowed.contains(CredentialType::SSH_KEY) {
                    Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
                } else if allowed.contains(CredentialType::DEFAULT) {
                    Cred::default()
                } else {
                    Cred::username(username_from_url.unwrap_or(""))
                }
            })?;
        Ok(cred)
    }
}

/// Credential provider that supplies a single HTTPS username+token pair.
/// For GitHub PATs, the username can be anything non-empty (GitHub ignores
/// it and authenticates by token); we default to `"x-access-token"`.
pub struct TokenCredentials {
    pub username: String,
    pub token: String,
}

impl TokenCredentials {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            username: "x-access-token".to_string(),
            token: token.into(),
        }
    }

    pub fn with_username(username: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            token: token.into(),
        }
    }
}

impl CredentialProvider for TokenCredentials {
    fn provide(
        &self,
        _url: &str,
        _username_from_url: Option<&str>,
        _allowed: CredentialType,
    ) -> Result<Cred> {
        Ok(Cred::userpass_plaintext(&self.username, &self.token)?)
    }
}
