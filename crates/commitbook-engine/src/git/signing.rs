//! SSH/GPG commit signing on top of libgit2.
//!
//! Reads `commit.gpgsign`, `gpg.format`, optional `user.signingkey`, and
//! `gpg.ssh.program` / `gpg.program` from libgit2's config (which honors
//! the same `~/.gitconfig` system git reads). When signing is enabled,
//! shells out to ssh-keygen or gpg with the unsigned commit object on
//! stdin and returns the armored signature.
//!
//! Mobile (iOS/Android): always returns Ok(None). Sandbox forbids exec.

#[cfg(not(any(target_os = "ios", target_os = "android")))]
use anyhow::Context;
use anyhow::Result;
use git2::Repository;

#[cfg(not(any(target_os = "ios", target_os = "android")))]
use std::io::Write;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use std::process::{Command, Stdio};

/// Sign an unsigned commit object's bytes per the repo's git config.
///
/// Returns `Ok(None)` if signing is disabled (`commit.gpgsign = false`,
/// the default), or if running on a platform where signing isn't supported.
/// Returns `Ok(Some(signature_armored))` on success.
pub fn sign_commit_object(repo: &Repository, unsigned_bytes: &[u8]) -> Result<Option<String>> {
    let config = match repo.config() {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let enabled = config.get_bool("commit.gpgsign").unwrap_or(false);
    if !enabled {
        return Ok(None);
    }

    let format = config
        .get_string("gpg.format")
        .unwrap_or_else(|_| "openpgp".to_string());

    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        let _ = (config, format, unsigned_bytes);
        return Ok(None);
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        match format.as_str() {
            "ssh" => sign_ssh(&config, unsigned_bytes).map(Some),
            "openpgp" | "gpg" => sign_gpg(&config, unsigned_bytes).map(Some),
            other => Err(anyhow::anyhow!(
                "Unsupported gpg.format '{other}': only 'ssh' and 'openpgp' are currently supported"
            )),
        }
    }
}

/// Resolve `user.signingkey` to a filesystem path `ssh-keygen -f` can read.
///
/// Git accepts a literal public key as the signing key (a `key::` prefix, or a
/// bare `ssh-ed25519 ...` value, as 1Password and similar agents produce) and
/// writes it to a temp file before signing. A plain value is treated as a path
/// (with `~/` expanded). The returned `NamedTempFile`, when present, must be
/// kept alive until signing completes.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn resolve_ssh_key_file(raw_key: &str) -> Result<(String, Option<tempfile::NamedTempFile>)> {
    let literal = raw_key
        .strip_prefix("key::")
        .map(str::to_string)
        .or_else(|| {
            let t = raw_key.trim();
            (t.starts_with("ssh-")
                || t.starts_with("ecdsa-")
                || t.starts_with("sk-ssh-")
                || t.starts_with("sk-ecdsa-"))
            .then(|| t.to_string())
        });

    match literal {
        Some(key) => {
            let mut f = tempfile::NamedTempFile::new()
                .context("Failed to create temp file for literal signing key")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(f.path(), std::fs::Permissions::from_mode(0o600))
                    .context("Failed to restrict temp signing key permissions")?;
            }
            writeln!(f, "{}", key.trim()).context("Failed to write temp signing key")?;
            f.flush().context("Failed to flush temp signing key")?;
            let path = f.path().to_string_lossy().into_owned();
            Ok((path, Some(f)))
        }
        None => Ok((expand_tilde(raw_key), None)),
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn sign_ssh(config: &git2::Config, unsigned_bytes: &[u8]) -> Result<String> {
    let raw_key = config
        .get_string("user.signingkey")
        .context("commit.gpgsign=true but user.signingkey not set")?;
    // Keep the temp file (if the key was a literal) alive until signing ends.
    let (key_path, _tmp_key) = resolve_ssh_key_file(&raw_key)?;

    let program = config
        .get_string("gpg.ssh.program")
        .unwrap_or_else(|_| "ssh-keygen".to_string());

    let mut child = Command::new(&program)
        .args(["-Y", "sign", "-n", "git", "-f", &key_path])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn {program} for signing"))?;
    child
        .stdin
        .as_mut()
        .context("ssh-keygen stdin missing")?
        .write_all(unsigned_bytes)
        .context("Failed to pipe commit bytes to ssh-keygen")?;

    let output = child
        .wait_with_output()
        .context("Failed to wait for ssh-keygen")?;
    if !output.status.success() {
        anyhow::bail!(
            "ssh-keygen sign failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let sig = String::from_utf8(output.stdout).context("ssh-keygen produced non-UTF8 sig")?;
    Ok(sig)
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn sign_gpg(config: &git2::Config, unsigned_bytes: &[u8]) -> Result<String> {
    let signing_key = config.get_string("user.signingkey").ok();
    let program = config
        .get_string("gpg.program")
        .unwrap_or_else(|_| "gpg".to_string());

    let mut command = Command::new(&program);
    command.args(gpg_arguments(signing_key.as_deref()));
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn {program} for signing"))?;
    child
        .stdin
        .as_mut()
        .context("gpg stdin missing")?
        .write_all(unsigned_bytes)
        .context("Failed to pipe commit bytes to gpg")?;

    let output = child.wait_with_output().context("Failed to wait for gpg")?;
    if !output.status.success() {
        anyhow::bail!(
            "gpg sign failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let sig = String::from_utf8(output.stdout).context("gpg produced non-UTF8 sig")?;
    Ok(sig)
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn gpg_arguments(signing_key: Option<&str>) -> Vec<String> {
    let mut arguments = vec![
        "--sign".to_string(),
        "--armor".to_string(),
        "--detach-sign".to_string(),
    ];
    if let Some(signing_key) = signing_key {
        arguments.push("--local-user".to_string());
        arguments.push(signing_key.to_string());
    }
    arguments
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn expand_tilde(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home)
                .join(stripped)
                .to_string_lossy()
                .into_owned();
        }
    }
    path.to_string()
}

#[cfg(test)]
#[path = "signing_tests.rs"]
mod tests;
