//! Persistent identity of one local clone, never a remote repository ID.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::config::local::{read_regular_text, write_regular_text_atomic};
use crate::config::LocalConfig;
use crate::state::RepoLock;

pub const FILE_NAME: &str = "CommitBook-ID.toml";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    #[serde(rename = "CommitBook-Id")]
    commitbook_id: String,
}

pub fn path(repo_root: &Path) -> PathBuf {
    LocalConfig::local_dir(repo_root).join(FILE_NAME)
}

pub fn validate(id: &str) -> Result<()> {
    if id.len() != 8
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("CommitBook-Id must contain exactly eight lowercase hexadecimal characters");
    }
    Ok(())
}

/// Read without creating directories or repairing files, including symlinked parents.
pub fn load(repo_root: &Path) -> Result<String> {
    for dir in [
        LocalConfig::commitbook_dir(repo_root),
        LocalConfig::local_dir(repo_root),
    ] {
        let metadata = std::fs::symlink_metadata(&dir).with_context(|| {
            format!(
                "Missing local identity directory {}; run commitbook init or register this clone",
                dir.display()
            )
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("Unsafe identity directory: {}", dir.display());
        }
    }
    let identity: Identity = toml::from_str(&read_regular_text(&path(repo_root)).context(
        "Cannot read CommitBook-ID.toml; run commitbook init or register this clone if missing",
    )?)
    .context("Invalid CommitBook-ID.toml; file was not changed")?;
    validate(&identity.commitbook_id)?;
    Ok(identity.commitbook_id)
}

/// Register a clone without changing its shared configuration or Git history.
pub fn ensure(repo_root: &Path) -> Result<String> {
    let lock = RepoLock::acquire(repo_root)?;
    ensure_locked(repo_root, &lock)
}

pub fn ensure_locked(repo_root: &Path, lock: &RepoLock) -> Result<String> {
    lock.ensure_matches(repo_root)?;
    match std::fs::symlink_metadata(path(repo_root)) {
        Ok(_) => return load(repo_root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("Inspect CommitBook-ID.toml"),
    }
    let canonical = repo_root.canonicalize()?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("OS randomness unavailable: {e}"))?;
    let mut hash = Sha256::new();
    hash.update(b"CommitBook local clone identity v1\0");
    hash.update(canonical.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(random);
    let commitbook_id = hex::encode(hash.finalize())[..8].to_string();
    let text = toml::to_string(&Identity {
        commitbook_id: commitbook_id.clone(),
    })?;
    write_regular_text_atomic(&path(repo_root), &text)?;
    Ok(commitbook_id)
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
