//! Persistent identity of one local clone, never a remote repository ID.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::local::{read_regular_text, write_regular_text_atomic};
use crate::config::LocalConfig;
use crate::state::RepoLock;

pub const FILE_NAME: &str = "commitbook_local_id.toml";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    commitbook_local_id: String,
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
        bail!("commitbook_local_id must contain exactly eight lowercase hexadecimal characters");
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
    reject_old_identity(repo_root)?;
    let identity: Identity = toml::from_str(&read_regular_text(&path(repo_root)).context(
        "Cannot read commitbook_local_id.toml; run commitbook init or register this clone if missing",
    )?)
    .context("Invalid commitbook_local_id.toml; file was not changed")?;
    validate(&identity.commitbook_local_id)?;
    Ok(identity.commitbook_local_id)
}

/// Register a clone without changing its shared configuration or Git history.
pub fn ensure(repo_root: &Path) -> Result<String> {
    let lock = RepoLock::acquire(repo_root)?;
    ensure_locked(repo_root, &lock)
}

pub fn ensure_locked(repo_root: &Path, lock: &RepoLock) -> Result<String> {
    lock.ensure_matches(repo_root)?;
    reject_old_identity(repo_root)?;
    if std::fs::symlink_metadata(path(repo_root)).is_ok() {
        return load(repo_root);
    }
    ensure_locked_with_id(repo_root, lock, &generate_candidate()?)
}

/// Candidate for a new clone directory; callers must check for collisions.
pub fn generate_candidate() -> Result<String> {
    let mut random = [0u8; 4];
    getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("OS randomness unavailable: {e}"))?;
    Ok(hex::encode(random))
}

pub fn ensure_locked_with_id(repo_root: &Path, lock: &RepoLock, id: &str) -> Result<String> {
    lock.ensure_matches(repo_root)?;
    reject_old_identity(repo_root)?;
    match std::fs::symlink_metadata(path(repo_root)) {
        Ok(_) => {
            let existing = load(repo_root)?;
            if existing != id {
                bail!("Local identity {existing} does not match reserved clone ID {id}");
            }
            return Ok(existing);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("Inspect commitbook_local_id.toml"),
    }
    validate(id)?;
    let commitbook_local_id = id.to_string();
    let text = toml::to_string(&Identity {
        commitbook_local_id: commitbook_local_id.clone(),
    })?;
    write_regular_text_atomic(&path(repo_root), &text)?;
    Ok(commitbook_local_id)
}

fn reject_old_identity(repo_root: &Path) -> Result<()> {
    let old = LocalConfig::local_dir(repo_root).join("CommitBook-ID.toml");
    if std::fs::symlink_metadata(&old).is_ok() {
        bail!(
            "Unsupported pre-launch identity at {}; remove it and register this clone again",
            old.display()
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
