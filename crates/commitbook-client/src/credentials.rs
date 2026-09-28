//! Native host-app credentials for libgit2. Secrets are never persisted here.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use commitbook_engine::platform::CredentialProvider;
use git2::{Config, Cred, CredentialType};

use crate::types::{GitCredentialCallback, GitCredentialKind, GitCredentialRequest};

pub struct HostCredentials {
    callback: Option<Arc<dyn GitCredentialCallback>>,
}

impl HostCredentials {
    pub fn new(callback: Option<Arc<dyn GitCredentialCallback>>) -> Self {
        Self { callback }
    }
}

impl CredentialProvider for HostCredentials {
    fn provide(
        &self,
        _config: &Config,
        url: &str,
        username_from_url: Option<&str>,
        allowed: CredentialType,
    ) -> Result<Cred> {
        let Some(callback) = &self.callback else {
            if allowed.contains(CredentialType::USERNAME) {
                if let Some(username) = username_from_url {
                    return Ok(Cred::username(username)?);
                }
            }
            if allowed.contains(CredentialType::DEFAULT) {
                return Ok(Cred::default()?);
            }
            bail!("Git remote requires credentials; register a GitCredentialCallback");
        };
        let request = GitCredentialRequest {
            remote_url: commitbook_engine::git::remote::credential_free_url(url)?,
            username_from_url: username_from_url.map(str::to_string),
            allow_default: allowed.contains(CredentialType::DEFAULT),
            allow_username: allowed.contains(CredentialType::USERNAME),
            allow_user_password: allowed.contains(CredentialType::USER_PASS_PLAINTEXT),
            allow_ssh_key: allowed.contains(CredentialType::SSH_KEY),
        };
        let callback = Arc::clone(callback);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("commitbook-git-credentials".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    callback.provide(request)
                }));
                let _ = sender.send(result);
            })
            .context("Start Git credential callback")?;
        let response = receiver
            .recv_timeout(Duration::from_secs(60))
            .context("Git credential callback failed or timed out")?
            .map_err(|_| anyhow::anyhow!("Git credential callback panicked"))?;
        if let Some(message) = response.error_message.as_deref() {
            bail!("Host Git credential callback failed: {message}");
        }
        match response.kind {
            GitCredentialKind::Default if allowed.contains(CredentialType::DEFAULT) => {
                Ok(Cred::default()?)
            }
            GitCredentialKind::Username if allowed.contains(CredentialType::USERNAME) => {
                let username = response
                    .username
                    .as_deref()
                    .or(username_from_url)
                    .context("Missing Git username")?;
                Ok(Cred::username(username)?)
            }
            GitCredentialKind::UserPassword
                if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) =>
            {
                let username = response
                    .username
                    .as_deref()
                    .context("Missing Git username")?;
                let password = response
                    .password
                    .as_deref()
                    .context("Missing Git password")?;
                Ok(Cred::userpass_plaintext(username, password)?)
            }
            GitCredentialKind::SshKey if allowed.contains(CredentialType::SSH_KEY) => {
                let username = response
                    .username
                    .as_deref()
                    .or(username_from_url)
                    .context("Missing SSH username")?;
                let private_key = response.private_key.as_deref().context("Missing SSH key")?;
                Ok(Cred::ssh_key_from_memory(
                    username,
                    response.public_key.as_deref(),
                    private_key,
                    response.passphrase.as_deref(),
                )?)
            }
            _ => bail!("Host returned a Git credential type the remote does not accept"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GitCredentialResponse;
    use std::sync::Mutex;

    struct Callback {
        request: Mutex<Option<GitCredentialRequest>>,
        response: Mutex<Option<GitCredentialResponse>>,
    }

    impl GitCredentialCallback for Callback {
        fn provide(&self, request: GitCredentialRequest) -> GitCredentialResponse {
            *self.request.lock().unwrap() = Some(request);
            self.response.lock().unwrap().take().unwrap()
        }
    }

    #[test]
    fn https_callback_receives_redacted_url_and_supplies_user_password() {
        let callback = Arc::new(Callback {
            request: Mutex::new(None),
            response: Mutex::new(Some(GitCredentialResponse {
                kind: GitCredentialKind::UserPassword,
                username: Some("user".into()),
                password: Some("secret".into()),
                public_key: None,
                private_key: None,
                passphrase: None,
                error_message: None,
            })),
        });
        let provider = HostCredentials::new(Some(callback.clone()));
        provider
            .provide(
                &Config::new().unwrap(),
                "https://user:secret@example.com/team/notes.git?token=hidden",
                None,
                CredentialType::USER_PASS_PLAINTEXT,
            )
            .unwrap();
        let request = callback.request.lock().unwrap().take().unwrap();
        assert_eq!(request.remote_url, "https://example.com/team/notes.git");
        assert!(request.allow_user_password);
        assert!(!request.allow_ssh_key);
    }

    #[test]
    fn ssh_callback_can_supply_an_in_memory_key() {
        let callback = Arc::new(Callback {
            request: Mutex::new(None),
            response: Mutex::new(Some(GitCredentialResponse {
                kind: GitCredentialKind::SshKey,
                username: None,
                password: None,
                public_key: None,
                private_key: Some("test key material".into()),
                passphrase: None,
                error_message: None,
            })),
        });
        let provider = HostCredentials::new(Some(callback.clone()));
        provider
            .provide(
                &Config::new().unwrap(),
                "ssh://git@example.com/team/notes.git",
                Some("git"),
                CredentialType::SSH_KEY,
            )
            .unwrap();
        assert!(
            callback
                .request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .allow_ssh_key
        );
    }
}
