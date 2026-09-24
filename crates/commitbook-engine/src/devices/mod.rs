//! Device registry: one committed file per device under
//! `.CommitBook/devices/<id>.toml`, so every device can see the others.
//!
//! A device only ever writes its own file, and only when it is registered or
//! renamed, so device files never conflict and never create commits on
//! their own. The device's id lives in the gitignored `local/device-id`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::local::{read_regular_text, write_regular_text_atomic};
use crate::config::{Auth, LocalConfig};

/// Length of a device id in hex characters.
const ID_LEN: usize = 8;
/// Longest accepted device name.
const MAX_NAME_LEN: usize = 64;

/// Contents of `.CommitBook/devices/<id>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub name: String,
    /// `macos`, `linux`, `ios`, `android`, or another `std::env::consts::OS`.
    pub platform: String,
    pub auth: Auth,
}

/// A device file together with its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub id: String,
    #[serde(flatten)]
    pub device: Device,
    /// True for the device running this process.
    pub this_device: bool,
}

/// Committed directory holding one file per device.
pub fn devices_dir(repo_root: &Path) -> PathBuf {
    LocalConfig::commitbook_dir(repo_root).join("devices")
}

/// Committed file for device `id`.
pub fn device_path(repo_root: &Path, id: &str) -> PathBuf {
    devices_dir(repo_root).join(format!("{id}.toml"))
}

/// Repository-relative path of device `id`'s file, for selective commits.
pub fn device_repo_path(id: &str) -> String {
    format!(".CommitBook/devices/{id}.toml")
}

fn id_path(repo_root: &Path) -> PathBuf {
    LocalConfig::local_dir(repo_root).join("device-id")
}

/// This platform's name as written to device files.
pub fn platform() -> &'static str {
    std::env::consts::OS
}

fn platform_label() -> &'static str {
    match platform() {
        "macos" => "macOS",
        "linux" => "Linux",
        "ios" => "iOS",
        "android" => "Android",
        "windows" => "Windows",
        other => other,
    }
}

/// Default name for a device that was not given one: platform plus the
/// start of its id, e.g. `macOS 7f3c`. Contains no personal data.
pub fn default_name(id: &str) -> String {
    format!("{} {}", platform_label(), &id[..id.len().min(4)])
}

/// This device's id, or `None` before it has registered.
pub fn this_device_id(repo_root: &Path) -> Result<Option<String>> {
    let path = id_path(repo_root);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            let id = read_regular_text(&path)?.trim().to_string();
            validate_id(&id).with_context(|| format!("Invalid device id in {}", path.display()))?;
            Ok(Some(id))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Failed to inspect {}", path.display())),
    }
}

/// This device's id and file, or `None` before it has registered.
pub fn this_device(repo_root: &Path) -> Result<Option<(String, Device)>> {
    let Some(id) = this_device_id(repo_root)? else {
        return Ok(None);
    };
    let path = device_path(repo_root, &id);
    if fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    Ok(Some((id, read_device(&path)?)))
}

/// Register this device, creating its id and file. A device that is already
/// registered is left unchanged; one whose file is missing (for example,
/// removed from another device) gets it written again.
/// Returns the device id.
pub fn register(repo_root: &Path, name: Option<&str>, auth: Auth) -> Result<String> {
    let name = name.map(validate_name).transpose()?;
    crate::state::ensure_local_layout(repo_root)?;
    let id = match this_device_id(repo_root)? {
        Some(id) => id,
        None => {
            let id = generate_id(repo_root);
            write_regular_text_atomic(&id_path(repo_root), &format!("{id}\n"))
                .context("Failed to save this device's id")?;
            id
        }
    };
    if fs::symlink_metadata(device_path(repo_root, &id)).is_ok() {
        return Ok(id);
    }
    let name = name.unwrap_or_else(|| default_name(&id));
    write_device(
        repo_root,
        &id,
        &Device {
            name,
            platform: platform().to_string(),
            auth,
        },
    )?;
    Ok(id)
}

/// Every device file, sorted by name. Unreadable files are skipped and
/// reported in the returned warnings so one bad file does not hide the rest.
pub fn list(repo_root: &Path) -> Result<(Vec<DeviceEntry>, Vec<String>)> {
    let this_id = this_device_id(repo_root).ok().flatten();
    let dir = devices_dir(repo_root);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()))
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", dir.display()))
        }
    };
    let mut devices = Vec::new();
    let mut warnings = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".toml"))
        else {
            continue;
        };
        if validate_id(id).is_err() {
            continue;
        }
        match read_device(&path) {
            Ok(device) => devices.push(DeviceEntry {
                id: id.to_string(),
                this_device: this_id.as_deref() == Some(id),
                device,
            }),
            Err(error) => warnings.push(format!("Skipping {}: {error:#}", path.display())),
        }
    }
    devices.sort_by(|a, b| a.device.name.cmp(&b.device.name).then(a.id.cmp(&b.id)));
    Ok((devices, warnings))
}

/// Rename this device.
pub fn rename(repo_root: &Path, name: &str) -> Result<()> {
    let name = validate_name(name)?;
    let (id, mut device) = this_device(repo_root)?
        .context("This device is not registered; run `commitbook init` first")?;
    device.name = name;
    write_device(repo_root, &id, &device)
}

/// Remove another device's file (e.g. a retired computer).
pub fn remove(repo_root: &Path, id: &str) -> Result<()> {
    validate_id(id)?;
    if this_device_id(repo_root)?.as_deref() == Some(id) {
        bail!("Refusing to remove this device ({id})");
    }
    let path = device_path(repo_root, id);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(&path).with_context(|| format!("Failed to remove {}", path.display()))
        }
        Ok(_) => bail!("Not a regular file: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("No device with id {id}")
        }
        Err(error) => Err(error).with_context(|| format!("Failed to inspect {}", path.display())),
    }
}

/// Auth value for a desktop device: `ssh` for an SSH remote, otherwise
/// `existing_local_repo` (desktop always uses the normal Git credentials).
pub fn desktop_auth(repo_root: &Path, remote: &str) -> Auth {
    match crate::git::remote::remote_identity(repo_root, remote) {
        Ok(identity) if identity.ssh => Auth::Ssh,
        _ => Auth::ExistingLocalRepo,
    }
}

fn read_device(path: &Path) -> Result<Device> {
    let content = read_regular_text(path)?;
    toml::from_str(&content).with_context(|| format!("Invalid device file {}", path.display()))
}

fn write_device(repo_root: &Path, id: &str, device: &Device) -> Result<()> {
    // Also verifies `.CommitBook/` is a real directory before creating in it.
    crate::state::ensure_local_layout(repo_root)?;
    let dir = devices_dir(repo_root);
    match fs::symlink_metadata(&dir) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!("Not a real directory: {}", dir.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&dir).with_context(|| format!("Failed to create {}", dir.display()))?
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect {}", dir.display()))
        }
    }
    let content = toml::to_string_pretty(device).context("Failed to serialize device")?;
    write_regular_text_atomic(&device_path(repo_root, id), &content)
        .with_context(|| format!("Failed to write device file for {id}"))
}

fn validate_id(id: &str) -> Result<()> {
    if id.len() == ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(())
    } else {
        bail!("Device ids are {ID_LEN} lowercase hex characters, got `{id}`")
    }
}

fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("Device name must not be empty");
    }
    if name.chars().any(char::is_control) {
        bail!("Device name must not contain control characters");
    }
    if name.chars().count() > MAX_NAME_LEN {
        bail!("Device name must be at most {MAX_NAME_LEN} characters");
    }
    Ok(name.to_string())
}

/// A fresh id: hash of time, process id, and repository path. Collisions
/// only matter between devices of one CommitBook, so 32 bits is plenty.
fn generate_id(repo_root: &Path) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(now.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(repo_root.to_string_lossy().as_bytes());
    hex::encode(hasher.finalize())[..ID_LEN].to_string()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
