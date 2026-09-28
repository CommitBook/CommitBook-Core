use anyhow::Result;
use std::path::Path;

use crate::config::{Auth, LocalConfig};

/// Initialize `.CommitBook/` inside a freshly-cloned (or existing) repo and
/// register this device.
///
/// Writes `.CommitBook/config.toml` when missing, ensures
/// `.CommitBook/local/` exists and is protected by `.CommitBook/.gitignore`,
/// and writes this device's `.CommitBook/devices/<id>.toml`. Does NOT
/// commit; the caller decides commit timing (typically `publish_metadata`
/// right after, so the `.CommitBook/` marker shows up on the remote).
///
/// Idempotent: an existing config keeps all its settings (including the
/// configured sync branch), and an already registered device is unchanged.
pub fn init_dot_commitbook(
    repo_root: &Path,
    name: &str,
    branch: &str,
    remote: &str,
    device_name: Option<&str>,
    auth: Auth,
) -> Result<()> {
    crate::state::prepare_local_state(repo_root)?;
    if !LocalConfig::exists(repo_root) {
        LocalConfig::new(name, branch, remote).save(repo_root)?;
    }
    LocalConfig::ensure_gitignore(repo_root)?;
    crate::devices::register(repo_root, device_name, auth)?;
    Ok(())
}
