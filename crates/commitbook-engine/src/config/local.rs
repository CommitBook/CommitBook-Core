use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::values::{Agent, CommitAgent, CommitMode, ConflictMode, LogKeep};

/// Format version of `config.toml`, bumped only when a release changes the
/// file layout incompatibly.
pub const CONFIG_SCHEMA: u32 = 1;

/// Default schedule for new CommitBooks.
pub const DEFAULT_SCHEDULE: &str = "1h";

/// Commented layout written at init. `save` fills in the real values, and
/// later saves edit values in place so user comments survive.
const TEMPLATE: &str = r#"# CommitBook settings. This file is committed and shared by every device.

[config]
schema = 1                    # file format, managed by CommitBook; do not edit

[commitbook]
name = ""                     # display name

[git]
branch = "main"
remote = "origin"             # provider, owner and repo are read from this remote's URL

[sync]
schedule = "1h"               # 5m | 15m | 30m | 1h | 2h | 4h | daily | 5-field cron expression

[commit]
mode = "timestamp"            # timestamp: "Writing <time>" message | ai: ask the agent
agent = "any"                 # any | claude | codex | copilot | gemini | cursor
                              # any = first installed of copilot, claude, codex, gemini, cursor

[conflicts]
mode = "both"                 # both: keep both versions without markers, you delete one
                              #   (.md, .markdown, .txt; other files fall back to manual)
                              # manual: leave <<<<<<< markers, sync stops until resolved
                              # ai: agent resolves | review: agent proposes, you approve
agent = "claude"              # claude | codex | copilot | gemini | cursor (used by ai and review)

[logs]
keep = "30d"                  # <N>d (e.g. 7d, 30d, 90d) | forever
"#;

/// `[config]`: metadata about the file itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigMeta {
    pub schema: u32,
}

/// `[commitbook]`: the CommitBook's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitBookSettings {
    /// Display name (e.g. "Personal Notes").
    pub name: String,
}

/// `[git]`: which branch of which remote this CommitBook syncs. Provider,
/// owner and repository name are read from the remote's URL, not stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitSettings {
    pub branch: String,
    pub remote: String,
}

/// `[sync]`: when scheduled syncs run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncSettings {
    /// What the user picked (`1h`, `daily`, or a cron expression); converted
    /// to cron only when installing the scheduler (`cron::to_cron`).
    pub schedule: String,
}

/// `[commit]`: how commit messages are written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitSettings {
    pub mode: CommitMode,
    pub agent: CommitAgent,
}

/// `[conflicts]`: what happens when a merge leaves conflicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictSettings {
    pub mode: ConflictMode,
    /// Used by the `ai` and `review` modes.
    pub agent: Agent,
}

/// `[logs]`: activity log retention.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogSettings {
    #[serde(default)]
    pub keep: LogKeep,
}

/// Shared configuration stored at `<repo>/.CommitBook/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalConfig {
    pub config: ConfigMeta,
    pub commitbook: CommitBookSettings,
    pub git: GitSettings,
    pub sync: SyncSettings,
    pub commit: CommitSettings,
    pub conflicts: ConflictSettings,
    #[serde(default)]
    pub logs: LogSettings,
}

impl LocalConfig {
    /// A new config with default settings.
    pub fn new(name: &str, branch: &str, remote: &str) -> Self {
        Self {
            config: ConfigMeta {
                schema: CONFIG_SCHEMA,
            },
            commitbook: CommitBookSettings {
                name: name.to_string(),
            },
            git: GitSettings {
                branch: branch.to_string(),
                remote: remote.to_string(),
            },
            sync: SyncSettings {
                schedule: DEFAULT_SCHEDULE.to_string(),
            },
            commit: CommitSettings {
                mode: CommitMode::Timestamp,
                agent: CommitAgent::Any,
            },
            conflicts: ConflictSettings {
                mode: ConflictMode::Both,
                agent: Agent::Claude,
            },
            logs: LogSettings::default(),
        }
    }

    /// Returns the .CommitBook directory path for a repo.
    pub fn commitbook_dir(repo_path: &Path) -> PathBuf {
        repo_path.join(".CommitBook")
    }

    /// Returns the config file path for a repo.
    pub fn config_path(repo_path: &Path) -> PathBuf {
        Self::commitbook_dir(repo_path).join("config.toml")
    }

    /// Returns the local state directory path for a repo.
    pub fn local_dir(repo_path: &Path) -> PathBuf {
        Self::commitbook_dir(repo_path).join("local")
    }

    /// Returns the logs directory path for a repo.
    pub fn logs_dir(repo_path: &Path) -> PathBuf {
        Self::local_dir(repo_path).join("logs")
    }

    /// Returns the lock file path for a repo.
    pub fn lock_path(repo_path: &Path) -> PathBuf {
        Self::local_dir(repo_path).join(".lock")
    }

    /// Returns the committed ignore file protecting device-local state.
    pub fn gitignore_path(repo_path: &Path) -> PathBuf {
        Self::commitbook_dir(repo_path).join(".gitignore")
    }

    /// Check if a repo has been set up with CommitBook.
    pub fn exists(repo_path: &Path) -> bool {
        is_real_directory(&Self::commitbook_dir(repo_path))
            && fs::symlink_metadata(Self::config_path(repo_path))
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    }

    /// Load the config from a repo. Never writes.
    pub fn load(repo_path: &Path) -> Result<Self> {
        require_real_directory(&Self::commitbook_dir(repo_path))?;
        let path = Self::config_path(repo_path);
        let content = read_regular_text(&path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        Self::parse(&content).with_context(|| format!("Invalid config: {}", path.display()))
    }

    /// Alias of `load`, kept for read-only callers such as status and
    /// registry scans.
    pub fn load_read_only(repo_path: &Path) -> Result<Self> {
        Self::load(repo_path)
    }

    /// Parse config text, rejecting files written in another format.
    pub fn parse(content: &str) -> Result<Self> {
        let table: toml::Table = toml::from_str(content).context("Not valid TOML")?;
        let schema = table
            .get("config")
            .and_then(toml::Value::as_table)
            .and_then(|config| config.get("schema"))
            .and_then(toml::Value::as_integer);
        if schema != Some(i64::from(CONFIG_SCHEMA)) {
            bail!(
                "Unsupported config.toml format; delete `.CommitBook/config.toml` and run `commitbook init`"
            );
        }
        let config: Self = table.try_into()?;
        if config.commitbook.name.trim().is_empty() {
            bail!("[commitbook] name must not be empty");
        }
        Ok(config)
    }

    /// Save the config. An existing file is edited in place, so comments and
    /// key order survive; a missing file starts from the commented template.
    pub fn save(&self, repo_path: &Path) -> Result<()> {
        crate::state::ensure_local_layout(repo_path)?;

        let path = Self::config_path(repo_path);
        let existing = match fs::symlink_metadata(&path) {
            Ok(_) => Some(read_regular_text(&path)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to inspect {}", path.display()))
            }
        };
        let mut document: toml_edit::DocumentMut = existing
            .as_deref()
            .unwrap_or(TEMPLATE)
            .parse()
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        self.write_values(&mut document);

        write_regular_text_atomic(&path, &document.to_string())
            .with_context(|| format!("Failed to write config: {}", path.display()))?;

        Ok(())
    }

    fn write_values(&self, document: &mut toml_edit::DocumentMut) {
        set_value(document, "config", "schema", i64::from(self.config.schema));
        set_value(
            document,
            "commitbook",
            "name",
            self.commitbook.name.as_str(),
        );
        set_value(document, "git", "branch", self.git.branch.as_str());
        set_value(document, "git", "remote", self.git.remote.as_str());
        set_value(document, "sync", "schedule", self.sync.schedule.as_str());
        set_value(document, "commit", "mode", self.commit.mode.as_str());
        set_value(document, "commit", "agent", self.commit.agent.as_str());
        set_value(document, "conflicts", "mode", self.conflicts.mode.as_str());
        set_value(
            document,
            "conflicts",
            "agent",
            self.conflicts.agent.as_str(),
        );
        set_value(document, "logs", "keep", self.logs.keep.to_string());
    }

    /// Initialize the .CommitBook directory structure with `config`.
    pub fn init(repo_path: &Path, config: &Self) -> Result<()> {
        crate::state::prepare_local_state(repo_path)?;
        config.save(repo_path)?;
        Self::ensure_gitignore(repo_path)?;
        Ok(())
    }

    /// Ensure `.CommitBook/local/` is ignored by the committed nested ignore
    /// file. The repository-root `.gitignore` is user-owned and never changed.
    pub fn ensure_gitignore(repo_path: &Path) -> Result<()> {
        crate::state::ensure_local_layout(repo_path)?;
        let path = Self::gitignore_path(repo_path);
        let content = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                read_regular_text(&path).context("Failed to read .CommitBook/.gitignore")?
            }
            Ok(_) => bail!(
                ".CommitBook/.gitignore is not a regular no-follow file: {}",
                path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error).context("Failed to inspect .CommitBook/.gitignore"),
        };
        let mut updated = content;
        let mut changed = false;
        let entry = "/local/";
        if !updated.lines().any(|line| line.trim() == entry) {
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push_str(entry);
            updated.push('\n');
            changed = true;
        }
        if changed {
            write_regular_text(&path, &updated)
                .context("Failed to write .CommitBook/.gitignore")?;
        }
        Ok(())
    }
}

/// Set `[table] key = value`, keeping the existing entry's comments and
/// spacing when the key is already present.
fn set_value(
    document: &mut toml_edit::DocumentMut,
    table: &str,
    key: &str,
    value: impl Into<toml_edit::Value>,
) {
    let mut value = value.into();
    let table = document
        .entry(table)
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .expect("config sections are tables");
    match table.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
        Some(existing) => {
            let mut decor = existing.decor().clone();
            // Keep a trailing comment in its column when the value's width
            // changes, e.g. `name = ""` becoming `name = "Personal Notes"`.
            let suffix = decor
                .suffix()
                .and_then(|raw| raw.as_str())
                .map(str::to_owned);
            if let Some(suffix) = suffix {
                let comment = suffix.trim_start_matches(' ');
                if comment.starts_with('#') {
                    let column = suffix.len() - comment.len() + bare_width(existing);
                    let padding = column.saturating_sub(bare_width(&value)).max(1);
                    decor.set_suffix(format!("{}{comment}", " ".repeat(padding)));
                }
            }
            *value.decor_mut() = decor;
            *existing = value;
        }
        None => {
            table.insert(key, toml_edit::Item::Value(value));
        }
    }
}

/// Width of a value as written, without surrounding whitespace or comments.
fn bare_width(value: &toml_edit::Value) -> usize {
    let mut bare = value.clone();
    bare.decor_mut().clear();
    bare.to_string().trim().chars().count()
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn require_real_directory(path: &Path) -> Result<()> {
    if is_real_directory(path) {
        Ok(())
    } else {
        bail!(
            "Path must be a real directory, not a symlink or special file: {}",
            path.display()
        )
    }
}

pub(crate) fn read_regular_text(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("Path is not a regular no-follow file: {}", path.display());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).with_context(|| {
        format!(
            "Failed to open {} without following symlinks",
            path.display()
        )
    })?;
    if !file.metadata()?.is_file() {
        bail!("Opened path is not a regular file: {}", path.display());
    }
    let mut content = String::new();
    file.read_to_string(&mut content)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content)
}

fn write_regular_text(path: &Path, content: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!("Refusing to replace non-regular path: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect {}", path.display()))
        }
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path).with_context(|| {
        format!(
            "Failed to open {} without following symlinks",
            path.display()
        )
    })?;
    if !file.metadata()?.is_file() {
        bail!("Opened path is not a regular file: {}", path.display());
    }
    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("Failed to sync {}", path.display()))?;
    Ok(())
}

/// Replace `path` atomically: write a sibling temporary file, flush and sync
/// it, carry over the existing permissions, then rename it over the
/// destination. An interrupted write leaves the previous file intact, and a
/// symlink or other non-regular destination is never followed or replaced.
pub(crate) fn write_regular_text_atomic(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Path has no parent directory: {}", path.display()))?;
    require_real_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Path has no file name: {}", path.display()))?;

    let existing_permissions = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Some(metadata.permissions())
        }
        Ok(_) => bail!("Refusing to replace non-regular path: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect {}", path.display()))
        }
    };

    // Leftovers from an interrupted earlier write live next to the committed
    // config, so sweep them before creating a fresh one.
    let temp_prefix = format!(".{file_name}.");
    let temp_suffix = ".tmp";
    sweep_stale_temp_files(parent, &temp_prefix, temp_suffix);

    let mut temp = tempfile::Builder::new()
        .prefix(&temp_prefix)
        .suffix(temp_suffix)
        .tempfile_in(parent)
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;
    temp.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write {}", temp.path().display()))?;
    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync {}", temp.path().display()))?;
    if let Some(permissions) = existing_permissions {
        temp.as_file()
            .set_permissions(permissions)
            .with_context(|| format!("Failed to preserve permissions of {}", path.display()))?;
    }
    // `persist` is a rename, so the destination is replaced as a directory
    // entry rather than written through. On failure the temporary file is
    // removed when the returned error drops it.
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}

fn sweep_stale_temp_files(parent: &Path, prefix: &str, suffix: &str) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let looks_like_temp = name.len() > prefix.len() + suffix.len()
            && name.starts_with(prefix)
            && name.ends_with(suffix);
        if looks_like_temp && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
