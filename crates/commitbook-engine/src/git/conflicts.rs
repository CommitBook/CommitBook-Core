//! Structured access to libgit2's index conflict stages.
//!
//! Conflict consumers must use these entries rather than parsing marker text
//! from the working tree.  The index preserves deletion sides, binary blobs,
//! and file modes that conflict markers cannot represent.

use anyhow::{bail, Context, Result};
use git2::{FileFavor, IndexEntry, IndexTime, MergeFileInput, MergeFileOptions, Oid, Repository};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use super::GitRepo;

const REGULAR_FILE: u32 = 0o100644;
const EXECUTABLE_FILE: u32 = 0o100755;
const SYMLINK: u32 = 0o120000;
const GITLINK: u32 = 0o160000;

/// One side of an unmerged index entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSide {
    pub oid: Oid,
    pub mode: u32,
    /// Raw blob bytes. Gitlinks have no blob and therefore use an empty value;
    /// their mode still distinguishes them from an empty regular file.
    pub content: Vec<u8>,
}

impl ConflictSide {
    pub fn text(&self) -> Option<&str> {
        if self.is_regular_file() && !self.content.contains(&0) {
            std::str::from_utf8(&self.content).ok()
        } else {
            None
        }
    }

    pub fn is_regular_file(&self) -> bool {
        matches!(self.mode, REGULAR_FILE | EXECUTABLE_FILE)
    }

    pub fn is_symlink(&self) -> bool {
        self.mode == SYMLINK
    }

    pub fn is_gitlink(&self) -> bool {
        self.mode == GITLINK
    }
}

/// Complete ancestor/local/remote index state for one conflicted path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitConflict {
    pub path: String,
    pub ancestor: Option<ConflictSide>,
    pub local: Option<ConflictSide>,
    pub remote: Option<ConflictSide>,
}

impl GitConflict {
    /// Text resolution is safe only when every present side is a UTF-8 regular
    /// file with no NUL bytes.
    pub fn is_binary_or_special(&self) -> bool {
        [&self.ancestor, &self.local, &self.remote]
            .into_iter()
            .flatten()
            .any(|side| side.text().is_none())
    }

    pub fn classification(&self) -> &'static str {
        if [&self.ancestor, &self.local, &self.remote]
            .into_iter()
            .flatten()
            .any(|side| side.is_gitlink())
        {
            "gitlink"
        } else if [&self.ancestor, &self.local, &self.remote]
            .into_iter()
            .flatten()
            .any(|side| side.is_symlink())
        {
            "symlink"
        } else if self.is_binary_or_special() {
            "binary"
        } else {
            match (
                self.ancestor.is_some(),
                self.local.is_some(),
                self.remote.is_some(),
            ) {
                (false, true, true) => "add_add",
                (true, false, true) => "delete_modify",
                (true, true, false) => "modify_delete",
                _ => "content",
            }
        }
    }

    pub fn ancestor_text(&self) -> Option<&str> {
        self.ancestor.as_ref().and_then(ConflictSide::text)
    }

    pub fn local_text(&self) -> Option<&str> {
        self.local.as_ref().and_then(ConflictSide::text)
    }

    pub fn remote_text(&self) -> Option<&str> {
        self.remote.as_ref().and_then(ConflictSide::text)
    }
}

impl GitRepo {
    /// Read all unmerged entries directly from index stages 1/2/3.
    pub fn list_conflicts_structured(&self) -> Result<Vec<GitConflict>> {
        let repository = Repository::open(self.path())
            .with_context(|| format!("Failed to open {}", self.path().display()))?;
        let index = repository.index().context("Failed to get index")?;
        if !index.has_conflicts() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        for item in index.conflicts().context("Failed to iterate conflicts")? {
            let conflict = item.context("Failed to read conflict entry")?;
            let path_bytes = conflict
                .our
                .as_ref()
                .or(conflict.their.as_ref())
                .or(conflict.ancestor.as_ref())
                .map(|entry| entry.path.as_slice())
                .context("Conflict has no path")?;
            let path = std::str::from_utf8(path_bytes)
                .context("Conflict path is not valid UTF-8")?
                .to_string();
            validate_relative_path(&path)?;

            result.push(GitConflict {
                path,
                ancestor: load_side(&repository, conflict.ancestor.as_ref())?,
                local: load_side(&repository, conflict.our.as_ref())?,
                remote: load_side(&repository, conflict.their.as_ref())?,
            });
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(result)
    }

    pub fn find_conflict(&self, path: &str) -> Result<Option<GitConflict>> {
        Ok(self
            .list_conflicts_structured()?
            .into_iter()
            .find(|conflict| conflict.path == path))
    }

    /// Resolve with an exact index side. Passing `None` stages deletion.
    pub fn resolve_conflict_with_side(
        &self,
        path: &str,
        side: Option<&ConflictSide>,
    ) -> Result<()> {
        let conflict = self
            .find_conflict(path)?
            .with_context(|| format!("Conflict {path} not found"))?;
        if let Some(side) = side {
            if ![&conflict.ancestor, &conflict.local, &conflict.remote]
                .into_iter()
                .flatten()
                .any(|candidate| candidate == side)
            {
                bail!("Resolution side does not belong to conflict {path}");
            }
        }
        let deleting_gitlink = side.is_none()
            && [&conflict.ancestor, &conflict.local, &conflict.remote]
                .into_iter()
                .flatten()
                .any(|candidate| candidate.is_gitlink());
        self.apply_resolution(
            path,
            side.map(|s| (&s.content[..], s.mode, s.oid)),
            deleting_gitlink,
            true,
        )
    }

    /// Resolve a text conflict with caller-provided UTF-8 content.
    pub fn resolve_conflict_with_text(&self, path: &str, content: &str) -> Result<()> {
        let conflict = self
            .find_conflict(path)?
            .with_context(|| format!("Conflict {path} not found"))?;
        if conflict.is_binary_or_special() {
            bail!("Conflict {path} is binary or special and cannot use text resolution");
        }
        if has_conflict_markers(content) {
            bail!("Resolution for {path} still contains conflict markers");
        }
        let mode = conflict
            .local
            .as_ref()
            .or(conflict.remote.as_ref())
            .or(conflict.ancestor.as_ref())
            .map(|side| side.mode)
            .unwrap_or(REGULAR_FILE);
        let repository = Repository::open(self.path())?;
        let oid = repository.blob(content.as_bytes())?;
        self.apply_resolution(path, Some((content.as_bytes(), mode, oid)), false, false)
    }

    /// Resolve a text conflict whose conflicting hunks only add lines on both
    /// sides (the ancestor section of every hunk is blank), keeping both
    /// additions with the local side first. Returns `false` and changes
    /// nothing for any other shape, e.g. when an existing line was edited.
    pub fn try_resolve_append_only(&self, path: &str) -> Result<bool> {
        let Some(conflict) = self.find_conflict(path)? else {
            return Ok(false);
        };
        let Some(merged) = append_only_union(&conflict)? else {
            return Ok(false);
        };
        self.resolve_conflict_with_text(path, &merged)?;
        Ok(true)
    }

    fn apply_resolution(
        &self,
        path: &str,
        resolution: Option<(&[u8], u32, Oid)>,
        preserve_gitlink_worktree: bool,
        replace_final_symlink: bool,
    ) -> Result<()> {
        let rel = validate_relative_path(path)?;
        let repository = Repository::open(self.path())?;
        let workdir = repository
            .workdir()
            .context("Cannot resolve conflicts in a bare repository")?;
        let abs = workdir.join(&rel);
        ensure_safe_parent(workdir, &rel)?;

        match resolution {
            None if preserve_gitlink_worktree && is_real_directory(&abs) => {
                // Removing a gitlink from the index must not recursively remove
                // or reject the checked-out submodule directory. Git itself
                // leaves that worktree for the user to inspect/remove.
            }
            None => remove_worktree_entry(&abs)?,
            Some((content, SYMLINK, _)) => write_symlink(&abs, content)?,
            Some((_content, GITLINK, _)) => {
                // A gitlink is represented by its index OID. Leave an existing
                // submodule worktree alone; it is not a blob to materialize.
            }
            Some((content, mode, _)) => {
                if replace_final_symlink {
                    remove_final_symlink(&abs)?;
                }
                write_regular_nofollow(&abs, content)?;
                set_regular_mode(&abs, mode)?;
            }
        }

        let mut index = repository.index().context("Failed to get index")?;
        match resolution {
            None => index
                .remove_path(&rel)
                .with_context(|| format!("Failed to stage deletion of {path}"))?,
            Some((content, mode, oid)) => {
                // `Index::add` only replaces the same stage. Remove the
                // conflict stages first so the new stage-0 entry resolves it.
                index
                    .remove_path(&rel)
                    .with_context(|| format!("Failed to clear conflict stages for {path}"))?;
                let entry = IndexEntry {
                    ctime: IndexTime::new(0, 0),
                    mtime: IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode,
                    uid: 0,
                    gid: 0,
                    file_size: content.len() as u32,
                    id: oid,
                    flags: 0,
                    flags_extended: 0,
                    path: path.as_bytes().to_vec(),
                };
                index
                    .add(&entry)
                    .with_context(|| format!("Failed to stage resolution of {path}"))?;
            }
        }
        index.write().context("Failed to write index")?;
        Ok(())
    }
}

fn load_side(repository: &Repository, entry: Option<&IndexEntry>) -> Result<Option<ConflictSide>> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let content = if entry.mode == GITLINK {
        Vec::new()
    } else {
        repository
            .find_blob(entry.id)
            .with_context(|| format!("Failed to read conflict blob {}", entry.id))?
            .content()
            .to_vec()
    };
    Ok(Some(ConflictSide {
        oid: entry.id,
        mode: entry.mode,
        content,
    }))
}

fn validate_relative_path(path: &str) -> Result<PathBuf> {
    let rel = Path::new(path);
    if rel.as_os_str().is_empty() || rel.is_absolute() {
        bail!("Conflict path must be a non-empty relative path: {path}");
    }
    if rel
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("Conflict path contains an unsafe component: {path}");
    }
    Ok(rel.to_path_buf())
}

fn ensure_safe_parent(workdir: &Path, rel: &Path) -> Result<()> {
    let root = workdir
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize {}", workdir.display()))?;
    let mut current = root.clone();
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));
    for component in parent.components() {
        let Component::Normal(segment) = component else {
            bail!("Unsafe conflict path {}", rel.display());
        };
        current.push(segment);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!(
                        "Conflict path traverses a non-directory or symlink: {}",
                        current.display()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)
                    .with_context(|| format!("Failed to create {}", current.display()))?;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to inspect {}", current.display()))
            }
        }
        let canonical = current
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize {}", current.display()))?;
        if !canonical.starts_with(&root) {
            bail!("Conflict path escapes repository: {}", rel.display());
        }
    }
    Ok(())
}

fn remove_worktree_entry(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            bail!("Refusing to replace directory at {}", path.display())
        }
        Ok(_) => std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Failed to inspect {}", path.display())),
    }
}

fn is_real_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn remove_final_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::remove_file(path)
            .with_context(|| format!("Failed to replace symlink {}", path.display())),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("Failed to inspect {}", path.display())),
    }
}

fn write_regular_nofollow(path: &Path, content: &[u8]) -> Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!(
                "Refusing to write through non-regular path {}",
                path.display()
            );
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
    file.write_all(content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("Failed to sync {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_regular_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = if mode == EXECUTABLE_FILE {
        0o755
    } else {
        0o644
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(permissions))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_regular_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn write_symlink(path: &Path, target: &[u8]) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    remove_worktree_entry(path)?;
    symlink(OsStr::from_bytes(target), path)
        .with_context(|| format!("Failed to create symlink {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_symlink(path: &Path, _target: &[u8]) -> Result<()> {
    bail!(
        "Symlink conflict resolution is unsupported on this platform: {}",
        path.display()
    )
}

/// Marker width for the append-only probe merge, long enough that note text
/// cannot be mistaken for a marker line.
const PROBE_MARKER_SIZE: u16 = 32;

/// Union of both sides when every conflicting hunk has a blank ancestor
/// section; `None` for binary, delete, or edit conflicts.
pub(crate) fn append_only_union(conflict: &GitConflict) -> Result<Option<String>> {
    if conflict.is_binary_or_special() {
        return Ok(None);
    }
    let (Some(local), Some(remote)) = (conflict.local_text(), conflict.remote_text()) else {
        return Ok(None);
    };
    let ancestor = conflict.ancestor_text().unwrap_or("");

    let mut probe = MergeFileOptions::new();
    probe.style_diff3(true).marker_size(PROBE_MARKER_SIZE);
    let probe = merge_text(&conflict.path, ancestor, local, remote, &mut probe)?;
    if !conflict_bases_blank(&probe, PROBE_MARKER_SIZE as usize) {
        return Ok(None);
    }

    let mut union = MergeFileOptions::new();
    union.favor(FileFavor::Union);
    let merged = merge_text(&conflict.path, ancestor, local, remote, &mut union)?;
    // Note text that itself looks like markers would be rejected on staging;
    // leave such files to the configured resolver instead.
    Ok((!has_conflict_markers(&merged)).then_some(merged))
}

fn merge_text(
    path: &str,
    ancestor: &str,
    local: &str,
    remote: &str,
    options: &mut MergeFileOptions,
) -> Result<String> {
    let mut base = MergeFileInput::new();
    base.content(ancestor.as_bytes()).path(path);
    let mut ours = MergeFileInput::new();
    ours.content(local.as_bytes()).path(path);
    let mut theirs = MergeFileInput::new();
    theirs.content(remote.as_bytes()).path(path);
    let result = git2::merge_file(&base, &ours, &theirs, Some(options))
        .with_context(|| format!("Failed to merge {path}"))?;
    String::from_utf8(result.content().to_vec())
        .with_context(|| format!("Merged {path} is not valid UTF-8"))
}

/// True when `merged` (diff3 style) has at least one conflict and every
/// conflict's ancestor section is empty or whitespace-only.
fn conflict_bases_blank(merged: &str, marker_size: usize) -> bool {
    let start = "<".repeat(marker_size);
    let base = "|".repeat(marker_size);
    let split = "=".repeat(marker_size);
    let end = ">".repeat(marker_size);
    enum Region {
        Outside,
        Ours,
        Base,
        Theirs,
    }
    let mut region = Region::Outside;
    let mut conflicts = 0;
    for line in merged.lines() {
        region = match region {
            Region::Outside if line.starts_with(&start) => Region::Ours,
            Region::Outside => Region::Outside,
            Region::Ours if line.starts_with(&base) => Region::Base,
            // diff3 always emits an ancestor marker; its absence is unexpected.
            Region::Ours if line.starts_with(&split) => return false,
            Region::Ours => Region::Ours,
            Region::Base if line.starts_with(&split) => Region::Theirs,
            Region::Base if !line.trim().is_empty() => return false,
            Region::Base => Region::Base,
            Region::Theirs if line.starts_with(&end) => {
                conflicts += 1;
                Region::Outside
            }
            Region::Theirs => Region::Theirs,
        };
    }
    conflicts > 0 && matches!(region, Region::Outside)
}

pub fn has_conflict_markers(content: &str) -> bool {
    let mut has_start = false;
    let mut has_end = false;
    let mut has_base = false;
    for line in content.lines() {
        let line = line.trim_start();
        has_start |= line.starts_with("<<<<<<<");
        has_end |= line.starts_with(">>>>>>>");
        has_base |= line.starts_with("|||||||");
    }
    // A standalone `=======` is valid Markdown (for example a setext
    // heading), so only the unambiguous marker lines are rejected.
    has_start || has_end || has_base
}

#[cfg(test)]
#[path = "conflicts_tests.rs"]
mod tests;
