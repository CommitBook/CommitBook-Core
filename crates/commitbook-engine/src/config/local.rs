use anyhow::{bail, Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn default_remote_name() -> String {
    "origin".to_string()
}

/// Git-specific settings for the repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSettings {
    pub auto_push: bool,
    pub branch: String,
    #[serde(default = "default_remote_name")]
    pub remote: String,
}

impl Default for GitSettings {
    fn default() -> Self {
        Self {
            auto_push: true,
            branch: "main".to_string(),
            remote: default_remote_name(),
        }
    }
}

/// Logging settings for the repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    pub level: String,
    #[serde(alias = "max_log_files")]
    pub max_log_days: u32,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            max_log_days: 30,
        }
    }
}

fn default_conflict_resolver() -> String {
    "manual".to_string()
}

fn default_auto_merge_appends() -> bool {
    true
}

/// Conflict-resolution settings.
///
/// `auto_merge_appends` (default on) first resolves conflicts where both
/// sides only added lines at the same place, keeping both additions.
/// `resolver` selects which AI CLI is invoked for the remaining structured
/// merge conflicts. Recognized values: `manual`, `claude`, `codex`,
/// `copilot`, `gemini`, `cursor`. `manual` (the default) preserves the
/// conflicted index for the user to resolve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictSettings {
    #[serde(default = "default_auto_merge_appends")]
    pub auto_merge_appends: bool,
    #[serde(default)]
    pub review_ai_resolutions: bool,
    #[serde(default = "default_conflict_resolver")]
    pub resolver: String,
}

impl Default for ConflictSettings {
    fn default() -> Self {
        Self {
            auto_merge_appends: default_auto_merge_appends(),
            resolver: default_conflict_resolver(),
            review_ai_resolutions: false,
        }
    }
}

fn default_ai_messages() -> bool {
    false
}

/// Commit-message settings.
///
/// `ai_messages = false` (the default) uses local `Writing <datetime>` text
/// without invoking AI CLIs. Opting in with `true` tries Copilot, Claude,
/// then Codex, falling back to the same timestamp message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSettings {
    #[serde(default = "default_ai_messages")]
    pub ai_messages: bool,
}

impl Default for CommitSettings {
    fn default() -> Self {
        Self {
            ai_messages: default_ai_messages(),
        }
    }
}

/// Settings identifying this clone as a CommitBook (a GitHub repo with
/// `.CommitBook/`). Optional in the schema for backwards compat with existing
/// notebooks that pre-date the FFI; the FFI client populates this when
/// initializing CommitBooks via `init_commitbook`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommitBookSettings {
    /// Display name (e.g. "Personal Notes").
    pub name: String,
    /// GitHub user/org owning the repo.
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// Provider: "github" | "gitlab" | "codeberg" | "generic_git".
    pub provider: String,
    /// Auth mode: "github_app" | "pat" | "ssh" | "existing_local_repo".
    pub mode: String,
}

fn default_config_version() -> String {
    "1".to_string()
}

fn canonicalize_v1_config_version(config_version: &str) -> Option<String> {
    if config_version == "1" || config_version.starts_with("1.") {
        Some("1".to_string())
    } else {
        None
    }
}

fn default_schedule() -> String {
    "0 * * * *".to_string()
}

fn default_created_at() -> String {
    "unknown".to_string()
}

fn normalize_legacy_config_table(table: &mut toml::map::Map<String, toml::Value>) -> bool {
    let mut changed = false;

    if !table.contains_key("config_version") {
        if let Some(version) = table.get("version").cloned() {
            table.insert("config_version".to_string(), version);
        } else {
            table.insert(
                "config_version".to_string(),
                toml::Value::String(default_config_version()),
            );
        }
        changed = true;
    }

    if table.remove("version").is_some() {
        changed = true;
    }

    if !table.contains_key("enabled") {
        table.insert("enabled".to_string(), toml::Value::Boolean(true));
        changed = true;
    }

    if !table.contains_key("schedule") {
        table.insert(
            "schedule".to_string(),
            toml::Value::String(default_schedule()),
        );
        changed = true;
    }

    if !table.contains_key("created_at") {
        table.insert(
            "created_at".to_string(),
            toml::Value::String(default_created_at()),
        );
        changed = true;
    }

    changed
}

/// Local configuration stored at <repo>/.CommitBook/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalConfig {
    #[serde(default = "default_config_version")]
    pub config_version: String,
    pub enabled: bool,
    pub schedule: String,
    pub created_at: String,
    #[serde(default)]
    pub git: GitSettings,
    #[serde(default)]
    pub logging: LoggingSettings,
    #[serde(default)]
    pub conflict: ConflictSettings,
    #[serde(default)]
    pub commit: CommitSettings,
    #[serde(default)]
    pub commitbook: Option<CommitBookSettings>,
    pub scheduler_id: Option<String>,
}

impl LocalConfig {
    /// Create a new default local config.
    pub fn new(schedule: &str) -> Self {
        Self {
            config_version: "1".to_string(),
            enabled: true,
            schedule: schedule.to_string(),
            created_at: crate::utils::datetime::now_iso(),
            git: GitSettings::default(),
            logging: LoggingSettings::default(),
            conflict: ConflictSettings::default(),
            commit: CommitSettings::default(),
            commitbook: None,
            scheduler_id: None,
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

    /// Migrate config to the latest version. Returns true if migration occurred.
    pub fn migrate(&mut self) -> bool {
        if let Some(canonical) = canonicalize_v1_config_version(&self.config_version) {
            if self.config_version != canonical {
                self.config_version = canonical;
                return true;
            }
        }

        false
    }

    fn validate_config_version(&self, path: &Path) -> Result<()> {
        if self.config_version == "1" {
            return Ok(());
        }

        bail!(
            "Unsupported local config version `{}` in {}",
            self.config_version,
            path.display()
        );
    }

    /// Load local config from a repo.
    pub fn load(repo_path: &Path) -> Result<Self> {
        Self::load_inner(repo_path, true)
    }

    /// Load and normalize local config in memory without rewriting it.
    /// Discovery paths use this so read-only registry scans never race a
    /// repository mutation that owns the repository lock.
    pub fn load_read_only(repo_path: &Path) -> Result<Self> {
        Self::load_inner(repo_path, false)
    }

    fn load_inner(repo_path: &Path, persist_repairs: bool) -> Result<Self> {
        require_real_directory(&Self::commitbook_dir(repo_path))?;
        let path = Self::config_path(repo_path);
        let content = read_regular_text(&path)
            .with_context(|| format!("Failed to read local config: {}", path.display()))?;

        let mut value: toml::Value = toml::from_str(&content)
            .with_context(|| format!("Failed to parse local config: {}", path.display()))?;
        let table = value.as_table_mut().ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to parse local config: {}: top-level TOML value must be a table",
                path.display()
            )
        })?;
        let remote_missing = table
            .get("git")
            .and_then(toml::Value::as_table)
            .and_then(|git| git.get("remote"))
            .is_none();
        let repaired = normalize_legacy_config_table(table);
        let normalized = toml::to_string(&value)
            .with_context(|| format!("Failed to normalize local config: {}", path.display()))?;

        let mut config: Self = toml::from_str(&normalized)
            .with_context(|| format!("Failed to parse local config: {}", path.display()))?;
        let migrated = config.migrate();
        config.validate_config_version(&path)?;
        let remote_inferred = if remote_missing {
            config.git.remote = infer_single_remote(repo_path)?;
            true
        } else {
            false
        };

        if persist_repairs && (repaired || migrated || remote_inferred) {
            config.save(repo_path)?;
        }

        Ok(config)
    }

    /// Save local config to the repo.
    pub fn save(&self, repo_path: &Path) -> Result<()> {
        crate::state::ensure_local_layout(repo_path)?;

        let path = Self::config_path(repo_path);
        let content =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize local config")?;

        write_regular_text_atomic(&path, &content)
            .with_context(|| format!("Failed to write local config: {}", path.display()))?;

        Ok(())
    }

    /// Initialize the .CommitBook directory structure.
    pub fn init(repo_path: &Path, schedule: &str) -> Result<Self> {
        crate::state::prepare_local_state(repo_path)?;

        let config = Self::new(schedule);
        config.save(repo_path)?;
        Self::ensure_gitignore(repo_path)?;

        Ok(config)
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

fn read_regular_text(path: &Path) -> Result<String> {
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
fn write_regular_text_atomic(path: &Path, content: &str) -> Result<()> {
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

fn infer_single_remote(repo_path: &Path) -> Result<String> {
    let repo = Repository::open(repo_path).with_context(|| {
        format!(
            "Local config is missing git.remote and {} is not a Git repository",
            repo_path.display()
        )
    })?;
    let remotes = repo.remotes().context("Failed to list Git remotes")?;
    match remotes.len() {
        1 => remotes
            .get(0)
            .context("The only configured Git remote name is not valid UTF-8")?
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("The only configured Git remote has no name")),
        0 => bail!(
            "Local config is missing git.remote and the repository has no remotes; add one and retry"
        ),
        count => bail!(
            "Local config is missing git.remote and the repository has {count} remotes; set git.remote in .CommitBook/config.toml"
        ),
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
