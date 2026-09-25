use anyhow::{bail, Context, Result};
use git2::{FetchOptions, PushOptions, RemoteCallbacks, Repository, Signature, StatusOptions};
use std::path::{Component, Path, PathBuf};

use crate::platform::{CredentialProvider, SystemCredentials};

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeadExpectation {
    symbolic_target: Option<String>,
    oid: Option<git2::Oid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FastForwardFailpoint {
    BeforeCheckout,
    BeforeRefPublication,
}

#[cfg(test)]
thread_local! {
    static FAST_FORWARD_FAILPOINT: std::cell::Cell<Option<FastForwardFailpoint>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(test)]
fn set_fast_forward_failpoint(failpoint: FastForwardFailpoint) {
    FAST_FORWARD_FAILPOINT.with(|slot| slot.set(Some(failpoint)));
}

#[cfg(test)]
thread_local! {
    static FAST_FORWARD_EXTERNAL_ADVANCE_TO: std::cell::Cell<Option<git2::Oid>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(test)]
fn set_fast_forward_external_advance_to(oid: git2::Oid) {
    FAST_FORWARD_EXTERNAL_ADVANCE_TO.with(|slot| slot.set(Some(oid)));
}

#[cfg(test)]
thread_local! {
    static FAST_FORWARD_SWITCH_HEAD_TO: std::cell::RefCell<Option<String>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
fn set_fast_forward_switch_head_to(reference: impl Into<String>) {
    FAST_FORWARD_SWITCH_HEAD_TO.with(|slot| *slot.borrow_mut() = Some(reference.into()));
}

fn fail_fast_forward_at(failpoint: FastForwardFailpoint) -> Result<()> {
    #[cfg(test)]
    {
        let injected = FAST_FORWARD_FAILPOINT.with(|slot| {
            if slot.get() == Some(failpoint) {
                slot.set(None);
                true
            } else {
                false
            }
        });
        if injected {
            bail!("Injected fast-forward failure at {failpoint:?}");
        }
    }
    #[cfg(not(test))]
    let _ = failpoint;
    Ok(())
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum PushFailpoint {
    NonFastForward,
    Auth,
}

#[cfg(test)]
thread_local! {
    static PUSH_FAILPOINT: std::cell::Cell<Option<PushFailpoint>> = const {
        std::cell::Cell::new(None)
    };
    static PUSH_ATTEMPTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn set_push_failpoint(failpoint: PushFailpoint) {
    PUSH_FAILPOINT.with(|slot| slot.set(Some(failpoint)));
    PUSH_ATTEMPTS.with(|attempts| attempts.set(0));
}

#[cfg(test)]
pub(crate) fn push_attempts() -> u32 {
    PUSH_ATTEMPTS.with(std::cell::Cell::get)
}

#[cfg(test)]
fn injected_push_failure() -> Option<git2::Error> {
    PUSH_ATTEMPTS.with(|attempts| attempts.set(attempts.get() + 1));
    PUSH_FAILPOINT.with(|slot| {
        slot.take().map(|failpoint| match failpoint {
            PushFailpoint::NonFastForward => git2::Error::new(
                git2::ErrorCode::NotFastForward,
                git2::ErrorClass::Net,
                "injected non-fast-forward",
            ),
            PushFailpoint::Auth => git2::Error::new(
                git2::ErrorCode::Auth,
                git2::ErrorClass::Net,
                "injected authentication failure",
            ),
        })
    })
}

#[cfg(test)]
thread_local! {
    static COMMIT_PUBLISH_ADVANCE_TO: std::cell::Cell<Option<git2::Oid>> = const {
        std::cell::Cell::new(None)
    };
}

#[cfg(test)]
fn set_commit_publish_advance_to(oid: git2::Oid) {
    COMMIT_PUBLISH_ADVANCE_TO.with(|slot| slot.set(Some(oid)));
}

#[cfg(test)]
thread_local! {
    static COMMIT_PUBLISH_SWITCH_HEAD_TO: std::cell::RefCell<Option<String>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
fn set_commit_publish_switch_head_to(reference: impl Into<String>) {
    COMMIT_PUBLISH_SWITCH_HEAD_TO.with(|slot| *slot.borrow_mut() = Some(reference.into()));
}

/// Wrapper around git2 for repository operations.
pub struct GitRepo {
    repo: Repository,
    path: PathBuf,
}

/// Summary of changes in a repository.
#[derive(Debug, Default, Clone)]
pub struct ChangesSummary {
    pub new_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub deleted_files: Vec<String>,
}

/// Outcome of `GitRepo::merge_from_remote`.
///
/// `Clean` covers up-to-date, fast-forward, and a successful 3-way merge with
/// no conflicts (in which case a merge commit is auto-created). `Conflicts`
/// signals unmerged paths in the index, the caller resolves them, stages,
/// and calls `finalize_merge_commit` to complete the merge.
#[derive(Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    Clean,
    Conflicts(Vec<String>),
}

impl ChangesSummary {
    pub fn is_empty(&self) -> bool {
        self.new_files.is_empty() && self.modified_files.is_empty() && self.deleted_files.is_empty()
    }

    pub fn total(&self) -> usize {
        self.new_files.len() + self.modified_files.len() + self.deleted_files.len()
    }

    /// Returns a short textual summary of changes for AI prompt context.
    pub fn to_summary_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.new_files.is_empty() {
            parts.push(format!("{} new", self.new_files.len()));
        }
        if !self.modified_files.is_empty() {
            parts.push(format!("{} modified", self.modified_files.len()));
        }
        if !self.deleted_files.is_empty() {
            parts.push(format!("{} deleted", self.deleted_files.len()));
        }
        if parts.is_empty() {
            "no changes".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// Returns a detailed summary with file names for AI context.
    pub fn to_detail_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.new_files.is_empty() {
            parts.push(format!(
                "{} new file(s): {}",
                self.new_files.len(),
                self.new_files.join(", ")
            ));
        }
        if !self.modified_files.is_empty() {
            parts.push(format!(
                "{} modified file(s): {}",
                self.modified_files.len(),
                self.modified_files.join(", ")
            ));
        }
        if !self.deleted_files.is_empty() {
            parts.push(format!(
                "{} deleted file(s): {}",
                self.deleted_files.len(),
                self.deleted_files.join(", ")
            ));
        }
        if parts.is_empty() {
            "No changes".to_string()
        } else {
            parts.join("; ")
        }
    }
}

impl GitRepo {
    /// Open an existing git repository at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let repo = Repository::open(path)
            .with_context(|| format!("Not a git repository: {}", path.display()))?;
        Ok(Self {
            repo,
            path: path.to_path_buf(),
        })
    }

    fn capture_head_expectation(&self) -> Result<HeadExpectation> {
        let head = self
            .repo
            .find_reference("HEAD")
            .context("Failed to read HEAD")?;
        let symbolic_target = head
            .symbolic_target()
            .context("HEAD symbolic target is not valid UTF-8")?
            .map(str::to_string);
        let direct_oid = head.target();
        drop(head);

        let oid = match symbolic_target.as_deref() {
            Some(reference) => self.reference_target_or_unborn(reference)?,
            None => direct_oid,
        };
        let expectation = HeadExpectation {
            symbolic_target,
            oid,
        };
        self.ensure_head_matches(&expectation, "capturing HEAD")?;
        Ok(expectation)
    }

    fn capture_head_on_branch(&self, branch: &str) -> Result<HeadExpectation> {
        let expectation = self.capture_head_expectation()?;
        let expected_reference = format!("refs/heads/{branch}");
        if expectation.symbolic_target.as_deref() != Some(expected_reference.as_str()) {
            let current = expectation
                .symbolic_target
                .as_deref()
                .and_then(|reference| reference.strip_prefix("refs/heads/"))
                .unwrap_or("detached HEAD");
            bail!(
                "Cannot operate on configured branch {branch:?} while HEAD is {current:?}. Check out the configured branch and retry."
            );
        }
        Ok(expectation)
    }

    fn reference_target_or_unborn(&self, reference_name: &str) -> Result<Option<git2::Oid>> {
        match self.repo.find_reference(reference_name) {
            Ok(reference) => reference
                .target()
                .map(Some)
                .with_context(|| format!("Reference {reference_name} is unexpectedly symbolic")),
            Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("Failed to read {reference_name}")),
        }
    }

    fn ensure_head_matches(&self, expected: &HeadExpectation, action: &str) -> Result<()> {
        let head = self
            .repo
            .find_reference("HEAD")
            .with_context(|| format!("Failed to re-read HEAD while {action}"))?;
        let symbolic_target = head
            .symbolic_target()
            .with_context(|| format!("HEAD symbolic target is not valid UTF-8 while {action}"))?
            .map(str::to_string);
        let direct_oid = head.target();
        drop(head);
        if symbolic_target != expected.symbolic_target {
            bail!("HEAD changed while {action}; retry the operation");
        }
        let oid = match symbolic_target.as_deref() {
            Some(reference) => self.reference_target_or_unborn(reference)?,
            None => direct_oid,
        };
        if oid != expected.oid {
            bail!("HEAD advanced while {action}; refusing to overwrite external Git work");
        }
        Ok(())
    }

    /// Check if a directory is a git repository.
    pub fn is_repo(path: &Path) -> bool {
        Repository::discover(path).is_ok()
    }

    /// Get a summary of all uncommitted changes.
    pub fn changes_summary(&self) -> Result<ChangesSummary> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .context("Failed to get repository status")?;

        let mut summary = ChangesSummary::default();

        for entry in statuses.iter() {
            let path = entry
                .path()
                .context("Changed path is not valid UTF-8")?
                .to_string();
            let status = entry.status();

            if status.is_wt_new() || status.is_index_new() {
                summary.new_files.push(path);
            } else if status.is_wt_modified() || status.is_index_modified() {
                summary.modified_files.push(path);
            } else if status.is_wt_deleted() || status.is_index_deleted() {
                summary.deleted_files.push(path);
            }
        }

        Ok(summary)
    }

    /// Stage specific paths (equivalent to `git add <paths...>`).
    pub fn stage_paths(&self, paths: &[String]) -> Result<()> {
        let mut index = self.repo.index().context("Failed to get index")?;
        for p in paths {
            index
                .add_path(Path::new(p))
                .with_context(|| format!("Failed to stage {p}"))?;
        }
        index.write().context("Failed to write index")?;
        Ok(())
    }

    /// Stage all changes (equivalent to `git add -A`).
    pub fn stage_all(&self) -> Result<()> {
        let mut index = self.repo.index().context("Failed to get index")?;
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .context("Failed to stage files")?;
        index
            .update_all(["*"].iter(), None)
            .context("Failed to update index for deletions")?;
        index.write().context("Failed to write index")?;
        Ok(())
    }

    /// Check if staged changes have actual content diffs (not just mtime changes).
    pub fn has_real_staged_changes(&self) -> Result<bool> {
        let head_tree = match self.repo.head() {
            Ok(head) => Some(head.peel_to_tree().context("Failed to peel HEAD to tree")?),
            Err(_) => None, // No HEAD yet, any staged content counts as change.
        };
        let diff = self
            .repo
            .diff_tree_to_index(head_tree.as_ref(), None, None)
            .context("Failed to diff HEAD to index")?;
        Ok(diff.deltas().count() > 0)
    }

    /// Create a commit with the given message. Honors `commit.gpgsign`.
    pub fn commit(&self, message: &str) -> Result<String> {
        let expected_head = self.capture_head_expectation()?;
        self.commit_with_expected_head(message, &expected_head)
    }

    /// Create a commit only if HEAD remains on the configured branch for the
    /// entire operation. This prevents a same-tip branch switch from publishing
    /// a sync commit onto a different branch.
    pub fn commit_on_branch(&self, message: &str, expected_branch: &str) -> Result<String> {
        let expected_head = self.capture_head_on_branch(expected_branch)?;
        self.commit_with_expected_head(message, &expected_head)
    }

    fn commit_with_expected_head(
        &self,
        message: &str,
        expected_head: &HeadExpectation,
    ) -> Result<String> {
        let mut index = self.repo.index().context("Failed to get index")?;
        let tree_oid = index.write_tree().context("Failed to write tree")?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .context("Failed to find tree")?;

        let sig = self
            .repo
            .signature()
            .or_else(|_| Signature::now("CommitBook", "commitbook@localhost"))
            .context("Failed to create signature")?;

        let parent_commit = expected_head
            .oid
            .map(|oid| self.repo.find_commit(oid))
            .transpose()
            .context("Failed to load expected HEAD commit")?;

        let parents: Vec<&git2::Commit> = parent_commit.iter().collect();

        let oid = self.commit_signed_or_plain(&sig, message, &tree, &parents, expected_head)?;

        // Return short hash
        Ok(oid.to_string()[..7].to_string())
    }

    /// Commit only the named working-tree paths without absorbing or
    /// disturbing unrelated entries already staged in the repository index.
    /// Returns `None` when those paths already match HEAD.
    pub fn commit_selected_paths(&self, paths: &[&str], message: &str) -> Result<Option<String>> {
        self.commit_selected_paths_inner(paths, message, None)
    }

    /// Commit selected paths only when HEAD names the configured branch.
    /// The branch ref is locked before reading HEAD and remains locked until
    /// the commit is published, so validation and publication are atomic.
    pub fn commit_selected_paths_on_branch(
        &self,
        paths: &[&str],
        message: &str,
        expected_branch: &str,
    ) -> Result<Option<String>> {
        self.commit_selected_paths_inner(paths, message, Some(expected_branch))
    }

    fn commit_selected_paths_inner(
        &self,
        paths: &[&str],
        message: &str,
        expected_branch: Option<&str>,
    ) -> Result<Option<String>> {
        let paths = paths
            .iter()
            .map(|path| validate_selected_path(path))
            .collect::<Result<Vec<_>>>()?;

        let (expected_head, mut branch_transaction) = if let Some(expected_branch) = expected_branch
        {
            let current_branch = self.current_branch()?;
            if current_branch != expected_branch {
                bail!(
                    "Cannot publish CommitBook metadata: checked out branch {:?} does not match configured branch {:?}. Check out the configured branch and retry.",
                    current_branch,
                    expected_branch
                );
            }
            let branch_ref = format!("refs/heads/{expected_branch}");
            let mut transaction = self
                .repo
                .transaction()
                .context("Failed to start metadata ref transaction")?;
            transaction
                .lock_ref("HEAD")
                .context("Failed to lock HEAD for metadata publication")?;
            transaction
                .lock_ref(&branch_ref)
                .with_context(|| format!("Failed to lock configured branch {branch_ref}"))?;
            let head = self
                .repo
                .find_reference("HEAD")
                .context("Failed to revalidate HEAD for metadata publication")?;
            if head
                .symbolic_target()
                .context("HEAD symbolic target is not valid UTF-8")?
                != Some(branch_ref.as_str())
            {
                bail!(
                    "Cannot publish CommitBook metadata because HEAD changed while locking the configured branch. Retry after checking out {:?}.",
                    expected_branch
                );
            }
            let oid = self.reference_target_or_unborn(&branch_ref)?;
            (
                HeadExpectation {
                    symbolic_target: Some(branch_ref.clone()),
                    oid,
                },
                Some((transaction, branch_ref)),
            )
        } else {
            (self.capture_head_expectation()?, None)
        };

        let parent = expected_head
            .oid
            .map(|oid| self.repo.find_commit(oid))
            .transpose()
            .context("Failed to load expected HEAD commit")?;
        let head_tree = parent
            .as_ref()
            .map(git2::Commit::tree)
            .transpose()
            .context("Failed to load expected HEAD tree")?;
        let mut selected_index = git2::Index::new().context("Failed to create temporary index")?;
        if let Some(tree) = &head_tree {
            selected_index
                .read_tree(tree)
                .context("Failed to seed temporary index from HEAD")?;
        }
        for path in &paths {
            self.update_index_path_from_worktree(&mut selected_index, path)?;
        }

        let tree_oid = selected_index
            .write_tree_to(&self.repo)
            .context("Failed to write selected-path tree")?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .context("Failed to load selected-path tree")?;
        let diff = self
            .repo
            .diff_tree_to_tree(head_tree.as_ref(), Some(&tree), None)
            .context("Failed to compare selected paths with HEAD")?;
        if diff.deltas().count() == 0 {
            self.align_index_paths(&paths, &selected_index)?;
            return Ok(None);
        }

        let sig = self
            .repo
            .signature()
            .or_else(|_| Signature::now("CommitBook", "commitbook@localhost"))
            .context("Failed to create signature")?;
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let oid = self.create_commit_object(&sig, message, &tree, &parents)?;
        if let Some((mut transaction, branch_ref)) = branch_transaction.take() {
            transaction
                .set_target(
                    &branch_ref,
                    oid,
                    Some(&sig),
                    "CommitBook: publish selected paths",
                )
                .with_context(|| format!("Failed to prepare metadata commit on {branch_ref}"))?;
            transaction
                .commit()
                .with_context(|| format!("Failed to publish metadata commit on {branch_ref}"))?;
        } else {
            self.publish_commit_to_head(oid, &expected_head, &sig)?;
        }

        // The real index remains user-owned throughout tree construction. Now
        // align just the committed paths with the new HEAD so they do not
        // appear as staged deletions while unrelated staged entries survive.
        self.align_index_paths(&paths, &selected_index)?;

        Ok(Some(oid.to_string()[..7].to_string()))
    }

    fn align_index_paths(&self, paths: &[PathBuf], selected: &git2::Index) -> Result<()> {
        let mut real_index = self.repo.index().context("Failed to get index")?;
        real_index.read(true)?;
        for path in paths {
            if let Some(entry) = selected.get_path(path, 0) {
                real_index.add(&entry)?;
            } else if real_index.get_path(path, 0).is_some() {
                real_index.remove_path(path)?;
            }
        }
        real_index
            .write()
            .context("Failed to update repository index")
    }

    fn update_index_path_from_worktree(&self, index: &mut git2::Index, path: &Path) -> Result<()> {
        let absolute = self.path.join(path);
        let metadata = match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if index.get_path(path, 0).is_some() {
                    index.remove_path(path).with_context(|| {
                        format!("Failed to remove selected path {}", path.display())
                    })?;
                }
                return Ok(());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to inspect {}", absolute.display()))
            }
        };

        let (data, mode) = if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&absolute)
                .with_context(|| format!("Failed to read symlink {}", absolute.display()))?;
            (target.as_os_str().as_encoded_bytes().to_vec(), 0o120000)
        } else if metadata.is_file() {
            let mode = selected_file_mode(&metadata);
            (
                std::fs::read(&absolute)
                    .with_context(|| format!("Failed to read {}", absolute.display()))?,
                mode,
            )
        } else {
            bail!("Selected path is not a file: {}", path.display());
        };

        let oid = if mode == 0o120000 {
            self.repo.blob(&data)?
        } else {
            use std::io::Write;
            let mut writer = self.repo.blob_writer(Some(path))?;
            writer.write_all(&data)?;
            writer.commit()?
        };
        let entry = git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            file_size: data.len() as u32,
            id: oid,
            flags: 0,
            flags_extended: 0,
            path: path.as_os_str().as_encoded_bytes().to_vec(),
        };
        index
            .add(&entry)
            .with_context(|| format!("Failed to add selected path {}", path.display()))?;
        Ok(())
    }

    /// Internal: build a commit object, optionally sign it, and publish it to
    /// HEAD. Centralizes the signing decision used by regular and merge commits.
    fn commit_signed_or_plain(
        &self,
        sig: &Signature,
        message: &str,
        tree: &git2::Tree,
        parents: &[&git2::Commit],
        expected_head: &HeadExpectation,
    ) -> Result<git2::Oid> {
        let oid = self.create_commit_object(sig, message, tree, parents)?;
        self.publish_commit_to_head(oid, expected_head, sig)?;
        Ok(oid)
    }

    fn create_commit_object(
        &self,
        sig: &Signature,
        message: &str,
        tree: &git2::Tree,
        parents: &[&git2::Commit],
    ) -> Result<git2::Oid> {
        // Unsigned fast path.
        let signing_enabled = self
            .repo
            .config()
            .ok()
            .and_then(|c| c.get_bool("commit.gpgsign").ok())
            .unwrap_or(false);
        if !signing_enabled {
            return self
                .repo
                .commit(None, sig, sig, message, tree, parents)
                .context("Failed to write commit");
        }

        // Signed path: build the buffer, sign it, and write the commit object.
        let unsigned_bytes = self
            .repo
            .commit_create_buffer(sig, sig, message, tree, parents)
            .context("Failed to build commit buffer")?;

        let signature = crate::git::signing::sign_commit_object(&self.repo, &unsigned_bytes)
            .context("Failed to sign commit")?;

        let unsigned_str =
            std::str::from_utf8(&unsigned_bytes).context("Commit buffer is not UTF-8")?;

        let oid = match signature {
            Some(sig_armored) => self
                .repo
                .commit_signed(unsigned_str, &sig_armored, Some("gpgsig"))
                .context("Failed to write signed commit")?,
            None => {
                // Signing was enabled but produced no signature (mobile
                // platform, or signing module declined). Fall through to
                // a plain commit, better than failing the sync.
                return self
                    .repo
                    .commit(None, sig, sig, message, tree, parents)
                    .context("Failed to write commit");
            }
        };

        Ok(oid)
    }

    fn publish_commit_to_head(
        &self,
        oid: git2::Oid,
        expected_head: &HeadExpectation,
        reflog_signature: &Signature,
    ) -> Result<()> {
        // Deterministically exercise an external ref advance between commit
        // object creation and publication. The compare-under-lock checks below
        // must preserve the externally-published tip.
        #[cfg(test)]
        COMMIT_PUBLISH_ADVANCE_TO.with(|slot| {
            if let Some(external_oid) = slot.take() {
                if let Some(branch_ref) = &expected_head.symbolic_target {
                    self.repo
                        .reference(branch_ref, external_oid, true, "test: external ref advance")
                        .unwrap();
                } else {
                    self.repo.set_head_detached(external_oid).unwrap();
                }
            }
        });
        #[cfg(test)]
        COMMIT_PUBLISH_SWITCH_HEAD_TO.with(|slot| {
            if let Some(reference) = slot.borrow_mut().take() {
                self.repo.set_head(&reference).unwrap();
            }
        });

        let mut transaction = self
            .repo
            .transaction()
            .context("Failed to start commit ref transaction")?;
        transaction
            .lock_ref("HEAD")
            .context("Failed to lock HEAD for commit publication")?;
        if let Some(branch_ref) = &expected_head.symbolic_target {
            transaction
                .lock_ref(branch_ref)
                .with_context(|| format!("Failed to lock {branch_ref} for commit publication"))?;
        }

        let locked_head = self
            .repo
            .find_reference("HEAD")
            .context("Failed to re-read locked HEAD")?;
        if locked_head
            .symbolic_target()
            .context("Locked HEAD symbolic target is not valid UTF-8")?
            != expected_head.symbolic_target.as_deref()
        {
            bail!("HEAD changed while publishing a commit; retry the operation");
        }

        let target_ref = expected_head.symbolic_target.as_deref().unwrap_or("HEAD");
        match (expected_head.oid, self.repo.find_reference(target_ref)) {
            (Some(expected), Ok(reference)) if reference.target() == Some(expected) => {}
            (None, Err(error)) if error.code() == git2::ErrorCode::NotFound => {}
            (Some(expected), Ok(reference)) => bail!(
                "Ref {target_ref} advanced from {} to {}; refusing to overwrite external Git work",
                expected,
                reference
                    .target()
                    .map(|target| target.to_string())
                    .unwrap_or_else(|| "a symbolic target".to_string())
            ),
            (Some(expected), Err(error)) => bail!(
                "Ref {target_ref} no longer points to expected commit {expected}: {error}"
            ),
            (None, Ok(_)) => bail!(
                "Ref {target_ref} was created while publishing the initial commit; refusing to overwrite external Git work"
            ),
            (None, Err(error)) => {
                return Err(error).with_context(|| format!("Failed to inspect {target_ref}"))
            }
        }

        transaction
            .set_target(
                target_ref,
                oid,
                Some(reflog_signature),
                "CommitBook: publish commit",
            )
            .with_context(|| format!("Failed to prepare commit publication on {target_ref}"))?;
        transaction
            .commit()
            .with_context(|| format!("Failed to publish commit on {target_ref}"))
    }

    /// Push the given branch to the remote using the system credential helper.
    pub fn push(&self, remote_name: &str, branch: &str) -> Result<()> {
        self.push_with(remote_name, branch, &SystemCredentials)
    }

    /// Push the given branch to the remote using the given credential provider.
    pub fn push_with(
        &self,
        remote_name: &str,
        branch: &str,
        creds: &dyn CredentialProvider,
    ) -> Result<()> {
        self.push_source_with(remote_name, branch, &format!("refs/heads/{branch}"), creds)
    }

    pub fn push_commit_with(
        &self,
        remote_name: &str,
        branch: &str,
        oid: &str,
        creds: &dyn CredentialProvider,
    ) -> Result<()> {
        let oid = git2::Oid::from_str(oid)?;
        self.repo.find_commit(oid)?;
        self.push_source_with(remote_name, branch, &oid.to_string(), creds)
    }

    fn push_source_with(
        &self,
        remote_name: &str,
        branch: &str,
        source: &str,
        creds: &dyn CredentialProvider,
    ) -> Result<()> {
        let mut remote = self
            .repo
            .find_remote(remote_name)
            .with_context(|| format!("Remote '{}' not found", remote_name))?;

        #[cfg(test)]
        if let Some(error) = injected_push_failure() {
            return Err(error.into());
        }

        let mut callbacks = RemoteCallbacks::new();
        let config = self.repo.config().context("Failed to read Git config")?;
        // Bound the callback: libgit2 re-invokes it on every rejection, so a
        // stateless provider that keeps returning bad credentials would loop
        // forever. Fail after 3 attempts instead.
        let attempts = std::rc::Rc::new(std::cell::Cell::new(0u32));
        callbacks.credentials(move |url, username_from_url, allowed| {
            let n = attempts.get();
            if n >= 3 {
                return Err(git2::Error::from_str(
                    "authentication failed: credentials rejected after 3 attempts",
                ));
            }
            attempts.set(n + 1);
            creds
                .provide(&config, url, username_from_url, allowed)
                .map_err(|e| git2::Error::from_str(&format!("credential provider failed: {e}")))
        });
        // Surface server-side rejections (branch protection, hook failures) as errors.
        let push_error = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
        let push_error_cb = push_error.clone();
        callbacks.push_update_reference(move |refname, status| {
            if let Some(msg) = status {
                *push_error_cb.borrow_mut() = Some(format!("{}: {}", refname, msg));
            }
            Ok(())
        });

        let mut opts = PushOptions::new();
        opts.remote_callbacks(callbacks);

        let refspec = format!("{source}:refs/heads/{branch}");
        remote
            .push(&[refspec.as_str()], Some(&mut opts))
            .with_context(|| format!("Failed to push {}/{}", remote_name, branch))?;

        // Drop opts (and thus the callback clone) before taking the value.
        drop(opts);
        if let Some(err) = push_error.borrow_mut().take() {
            return Err(push_rejection_error(&err));
        }
        Ok(())
    }

    /// Check if the repo has any remotes configured.
    pub fn has_remote(&self) -> bool {
        self.repo.remotes().map(|r| !r.is_empty()).unwrap_or(false)
    }

    /// Get the default remote name (usually "origin").
    pub fn default_remote_name(&self) -> Result<String> {
        let remotes = self.repo.remotes().context("Failed to list remotes")?;
        if remotes.is_empty() {
            bail!("No remotes configured");
        }
        remotes
            .get(0)
            .context("Default remote name is not valid UTF-8")?
            .map(str::to_string)
            .context("Default remote disappeared while reading it")
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> Result<String> {
        match self.repo.head() {
            Ok(head) => {
                if !head.is_branch() {
                    bail!("HEAD is detached; check out a branch and retry");
                }
                head.shorthand()
                    .map(str::to_string)
                    .context("Current branch name is not valid UTF-8")
            }
            Err(error)
                if matches!(
                    error.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                let head = self
                    .repo
                    .find_reference("HEAD")
                    .context("Failed to read unborn HEAD")?;
                let target = head
                    .symbolic_target()
                    .context("Unborn HEAD symbolic target is not valid UTF-8")?
                    .context("Unborn HEAD is not symbolic")?;
                target
                    .strip_prefix("refs/heads/")
                    .map(str::to_string)
                    .context("Unborn HEAD does not point to a local branch")
            }
            Err(error) => Err(error).context("Failed to get HEAD"),
        }
    }

    /// Get the repository path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the diff summary (stat) for AI prompt context, truncated.
    ///
    /// Produces a `git diff --stat HEAD` style summary vs HEAD (or vs an empty
    /// tree for the initial commit). Each changed file appears exactly once
    /// with its total delta, regardless of staging state. Covers working-tree
    /// and index in a single diff so the AI never sees a file twice.
    pub fn diff_summary(&self) -> Result<String> {
        let head_tree = self.repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), None)
            .context("Failed to compute diff")?;
        let stats = diff.stats().context("Failed to get diff stats")?;
        if stats.files_changed() == 0 {
            return Ok(String::new());
        }
        let buf = stats
            .to_buf(git2::DiffStatsFormat::FULL, 80)
            .context("Failed to format diff stats")?;
        let result = String::from_utf8_lossy(&buf).to_string();

        // Truncate to avoid overwhelming AI prompts.
        let lines: Vec<&str> = result.lines().collect();
        if lines.len() > 20 {
            let truncated: Vec<&str> = lines[..20].to_vec();
            return Ok(format!(
                "{}\n... and {} more files",
                truncated.join("\n"),
                lines.len() - 20
            ));
        }

        Ok(result.trim().to_string())
    }

    /// Fetch a specific branch from the remote using the system credential helper.
    pub fn fetch(&self, remote: &str, branch: &str) -> Result<()> {
        self.fetch_with(remote, branch, &SystemCredentials)
    }

    /// Fetch a specific branch from the remote using the given credential provider.
    pub fn fetch_with(
        &self,
        remote: &str,
        branch: &str,
        creds: &dyn CredentialProvider,
    ) -> Result<()> {
        let mut remote_obj = self
            .repo
            .find_remote(remote)
            .with_context(|| format!("Remote '{}' not found", remote))?;

        let mut callbacks = RemoteCallbacks::new();
        let config = self.repo.config().context("Failed to read Git config")?;
        // Bound the callback: libgit2 re-invokes it on every rejection, so a
        // stateless provider that keeps returning bad credentials would loop
        // forever. Fail after 3 attempts instead.
        let attempts = std::rc::Rc::new(std::cell::Cell::new(0u32));
        callbacks.credentials(move |url, username_from_url, allowed| {
            let n = attempts.get();
            if n >= 3 {
                return Err(git2::Error::from_str(
                    "authentication failed: credentials rejected after 3 attempts",
                ));
            }
            attempts.set(n + 1);
            creds
                .provide(&config, url, username_from_url, allowed)
                .map_err(|e| git2::Error::from_str(&format!("credential provider failed: {e}")))
        });

        let mut opts = FetchOptions::new();
        opts.remote_callbacks(callbacks);

        remote_obj
            .fetch(&[branch], Some(&mut opts), None)
            .with_context(|| format!("Failed to fetch {}/{}", remote, branch))
    }

    fn ff_changed_paths(&self, target_oid: git2::Oid) -> Result<Vec<PathBuf>> {
        let head_tree = self.repo.head().ok().and_then(|h| h.peel_to_tree().ok());
        let target_tree = self
            .repo
            .find_commit(target_oid)
            .context("Failed to load fast-forward target commit")?
            .tree()
            .context("Failed to load fast-forward target tree")?;

        // Files that differ between HEAD and the fast-forward target.
        let tree_diff = self
            .repo
            .diff_tree_to_tree(head_tree.as_ref(), Some(&target_tree), None)
            .context("Failed to diff HEAD against fast-forward target")?;
        let mut changed = std::collections::BTreeSet::new();
        tree_diff
            .foreach(
                &mut |delta, _| {
                    if let Some(path) = delta.old_file().path() {
                        changed.insert(path.to_path_buf());
                    }
                    if let Some(path) = delta.new_file().path() {
                        changed.insert(path.to_path_buf());
                    }
                    true
                },
                None,
                None,
                None,
            )
            .context("Failed to walk fast-forward diff")?;
        Ok(changed.into_iter().collect())
    }

    /// Paths changed by the target that also carry any staged, unstaged, or
    /// untracked local state. A non-empty result means checkout would clobber
    /// user work and must be refused.
    fn ff_dirty_conflicts(&self, changed: &[PathBuf]) -> Result<Vec<String>> {
        if changed.is_empty() {
            return Ok(Vec::new());
        }
        let mut opts = git2::StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(true)
            .recurse_ignored_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);
        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .context("Failed to read working-tree status")?;
        let mut conflicts = std::collections::BTreeSet::new();
        for entry in statuses.iter() {
            if entry.status() == git2::Status::CURRENT {
                continue;
            }
            let mut candidates = Vec::new();
            let path = entry
                .path()
                .context("Working-tree status path is not valid UTF-8")?;
            candidates.push(PathBuf::from(path));
            for delta in [entry.head_to_index(), entry.index_to_workdir()]
                .into_iter()
                .flatten()
            {
                if let Some(path) = delta.old_file().path() {
                    candidates.push(path.to_path_buf());
                }
                if let Some(path) = delta.new_file().path() {
                    candidates.push(path.to_path_buf());
                }
            }
            for path in candidates {
                if changed.iter().any(|target| paths_overlap(target, &path)) {
                    let path = path
                        .to_str()
                        .context("Conflicting working-tree path is not valid UTF-8")?;
                    conflicts.insert(path.to_string());
                }
            }
        }
        Ok(conflicts.into_iter().collect())
    }

    /// Advance the current branch after a path-limited safe checkout. The
    /// branch ref stays locked and unchanged until checkout succeeds. If ref
    /// publication fails, the affected worktree/index paths are restored to
    /// the original tree.
    fn fast_forward_to(
        &self,
        target_oid: git2::Oid,
        expected_head: &HeadExpectation,
    ) -> Result<()> {
        let head_name = expected_head
            .symbolic_target
            .as_deref()
            .context("HEAD is detached")?;
        let head_oid = expected_head.oid.context("HEAD is unborn")?;
        let head_commit = self
            .repo
            .find_commit(head_oid)
            .context("Failed to load expected HEAD commit")?;

        #[cfg(test)]
        FAST_FORWARD_EXTERNAL_ADVANCE_TO.with(|slot| {
            if let Some(external_oid) = slot.take() {
                self.repo
                    .reference(
                        head_name,
                        external_oid,
                        true,
                        "test: external fast-forward race",
                    )
                    .unwrap();
            }
        });
        #[cfg(test)]
        FAST_FORWARD_SWITCH_HEAD_TO.with(|slot| {
            if let Some(reference) = slot.borrow_mut().take() {
                self.repo.set_head(&reference).unwrap();
            }
        });

        let mut transaction = self
            .repo
            .transaction()
            .context("Failed to start fast-forward ref transaction")?;
        transaction
            .lock_ref("HEAD")
            .context("Failed to lock HEAD for fast-forward")?;
        transaction
            .lock_ref(head_name)
            .with_context(|| format!("Failed to lock {head_name}"))?;
        let locked_head = self
            .repo
            .find_reference("HEAD")
            .context("Failed to re-read locked HEAD")?;
        if locked_head
            .symbolic_target()
            .context("Locked HEAD symbolic target is not valid UTF-8")?
            != Some(head_name)
        {
            bail!("HEAD changed while preparing fast-forward; retry sync");
        }
        let locked_branch = self
            .repo
            .find_reference(head_name)
            .with_context(|| format!("Failed to re-read locked branch {head_name}"))?;
        if locked_branch.target() != Some(head_commit.id()) {
            bail!(
                "Branch {head_name} advanced while preparing fast-forward; refusing to overwrite external Git work"
            );
        }
        drop(locked_branch);
        drop(locked_head);
        if head_commit.id() != target_oid
            && !self
                .repo
                .graph_descendant_of(target_oid, head_commit.id())
                .context("Failed to revalidate fast-forward ancestry")?
        {
            bail!(
                "Branch {head_name} is no longer an ancestor of the fetched target; refusing a sideways or backward ref update"
            );
        }

        let changed = self.ff_changed_paths(target_oid)?;
        let clobbered = self.ff_dirty_conflicts(&changed)?;
        if !clobbered.is_empty() {
            bail!(
                "Fast-forward blocked by uncommitted local changes to: {}. Commit or stash them and re-run sync.",
                clobbered.join(", ")
            );
        }

        let target = self
            .repo
            .find_commit(target_oid)
            .context("Failed to load fast-forward target commit")?;
        let reflog_signature = self
            .repo
            .signature()
            .or_else(|_| Signature::now("CommitBook", "commitbook@localhost"))
            .context("Failed to create fast-forward reflog signature")?;
        fail_fast_forward_at(FastForwardFailpoint::BeforeCheckout)?;
        let snapshots = changed
            .iter()
            .map(|path| WorktreeSnapshot::capture(&self.path.join(path)))
            .collect::<Result<Vec<_>>>()?;
        let index_path = self.repo.path().join("index");
        let original_index = std::fs::read(&index_path).context("Failed to snapshot index")?;
        let publish = (|| -> Result<()> {
            if !changed.is_empty() {
                let mut checkout = git2::build::CheckoutBuilder::new();
                checkout.safe().disable_pathspec_match(true);
                for path in &changed {
                    checkout.path(path);
                }
                self.repo
                    .checkout_tree(target.as_object(), Some(&mut checkout))
                    .context("Failed to check out fast-forward target")?;
            }

            transaction
                .set_target(
                    head_name,
                    target_oid,
                    Some(&reflog_signature),
                    "CommitBook: fast-forward",
                )
                .context("Failed to prepare fast-forward branch ref")?;
            fail_fast_forward_at(FastForwardFailpoint::BeforeRefPublication)?;
            transaction
                .commit()
                .context("Failed to publish fast-forward branch ref")?;
            Ok(())
        })();
        if let Err(error) = publish {
            let mut failures = Vec::new();
            for (path, snapshot) in changed.iter().zip(&snapshots) {
                if let Err(restore_error) = snapshot.restore(&self.path.join(path)) {
                    failures.push(format!("{}: {restore_error:#}", path.display()));
                }
            }
            if let Err(restore_error) = restore_index(&index_path, &original_index) {
                failures.push(format!("index: {restore_error:#}"));
            }
            if !failures.is_empty() {
                bail!("Fast-forward failed ({error:#}); recovery incomplete for {}; repair these paths before retrying sync", failures.join(", "));
            }
            return Err(error).context("Fast-forward failed; original worktree and index restored");
        }
        Ok(())
    }

    /// Attempt a fast-forward-only merge of the given ref into HEAD.
    ///
    /// Returns `Ok(true)` if HEAD was advanced (or already at the target),
    /// `Ok(false)` if the merge would not be fast-forward. Other failures
    /// bubble up as errors.
    pub fn merge_ff_only(&self, refname: &str) -> Result<bool> {
        let expected_head = self.capture_head_expectation()?;
        let target_oid = self
            .repo
            .revparse_single(refname)
            .with_context(|| format!("Failed to resolve ref '{}'", refname))?
            .id();

        let head_oid = expected_head.oid.context("HEAD has no target")?;

        if head_oid == target_oid {
            self.ensure_head_matches(&expected_head, "checking fast-forward state")?;
            return Ok(true); // Already at target.
        }

        // HEAD must be an ancestor of target for a fast-forward to be possible.
        if !self
            .repo
            .graph_descendant_of(target_oid, head_oid)
            .context("Failed to compute ancestry")?
        {
            return Ok(false);
        }

        self.fast_forward_to(target_oid, &expected_head)?;

        Ok(true)
    }

    /// Count commits ahead and behind between two refs.
    ///
    /// Returns `(ahead, behind)`: how many commits `local` has that `remote` doesn't
    /// (ahead) and vice versa (behind).
    pub fn ahead_behind(&self, local: &str, remote: &str) -> Result<(u32, u32)> {
        let local_oid = self
            .repo
            .revparse_single(local)
            .with_context(|| format!("Failed to resolve ref '{}'", local))?
            .id();
        let remote_oid = self
            .repo
            .revparse_single(remote)
            .with_context(|| format!("Failed to resolve ref '{}'", remote))?
            .id();
        let (ahead, behind) = self
            .repo
            .graph_ahead_behind(local_oid, remote_oid)
            .context("Failed to compute ahead/behind counts")?;
        Ok((ahead as u32, behind as u32))
    }

    /// Read a file's content at a specific ref (commit/branch/tag).
    pub fn show_file_at_ref(&self, refname: &str, path: &str) -> Result<String> {
        let obj = self
            .repo
            .revparse_single(refname)
            .with_context(|| format!("Failed to resolve ref '{}'", refname))?;
        let tree = obj
            .peel_to_tree()
            .with_context(|| format!("Ref '{}' does not point to a tree", refname))?;
        let entry = tree
            .get_path(Path::new(path))
            .with_context(|| format!("Path '{}' not found at ref '{}'", path, refname))?;
        let object = entry
            .to_object(&self.repo)
            .with_context(|| format!("Failed to resolve object at {}:{}", refname, path))?;
        let blob = object
            .peel_to_blob()
            .with_context(|| format!("{}:{} is not a blob", refname, path))?;
        Ok(String::from_utf8_lossy(blob.content()).to_string())
    }

    /// List all file paths reachable from a ref.
    pub fn ls_tree_files(&self, refname: &str) -> Result<Vec<String>> {
        let obj = self
            .repo
            .revparse_single(refname)
            .with_context(|| format!("Failed to resolve ref '{}'", refname))?;
        let tree = obj
            .peel_to_tree()
            .with_context(|| format!("Ref '{}' does not point to a tree", refname))?;

        let mut files = Vec::new();
        let mut invalid_name = None;
        let walk_result = tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                let name = match entry.name() {
                    Ok(name) => name,
                    Err(error) => {
                        invalid_name = Some(error);
                        return git2::TreeWalkResult::Abort;
                    }
                };
                let path = if dir.is_empty() {
                    name.to_string()
                } else {
                    format!("{}{}", dir, name)
                };
                files.push(path);
            }
            git2::TreeWalkResult::Ok
        });
        if let Some(error) = invalid_name {
            return Err(error).context("Tree entry name is not valid UTF-8");
        }
        walk_result.context("Failed to walk tree")?;

        Ok(files)
    }

    /// Resolve a ref to its full SHA.
    pub fn rev_parse(&self, refname: &str) -> Result<String> {
        let obj = self
            .repo
            .revparse_single(refname)
            .with_context(|| format!("Failed to resolve ref '{}'", refname))?;
        Ok(obj.id().to_string())
    }

    /// SHA of the most recent commit reachable from HEAD that changed `path`.
    ///
    /// Equivalent to `git log -1 --format=%H -- <path>`. Returns `None` when
    /// the path has no committed history (e.g. it exists only in the working
    /// tree) or when HEAD is unborn.
    pub fn last_commit_touching(&self, path: &str) -> Result<Option<String>> {
        if self.repo.head().is_err() {
            return Ok(None); // Unborn branch: nothing committed yet.
        }
        let target = Path::new(path);
        let mut revwalk = self.repo.revwalk().context("Failed to create revwalk")?;
        revwalk
            .push_head()
            .context("Failed to push HEAD to revwalk")?;
        // Topological ordering guarantees children are visited before their
        // parents, so the first matching commit is the most recent one to touch
        // the path. TIME alone is unstable when commits share a timestamp and
        // can otherwise yield an ancestor before its descendant.
        revwalk
            .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
            .context("Failed to set revwalk sorting")?;

        for oid in revwalk {
            let oid = oid.context("Failed to read revwalk entry")?;
            let commit = self
                .repo
                .find_commit(oid)
                .context("Failed to find commit")?;
            let blob = commit
                .tree()
                .ok()
                .and_then(|t| t.get_path(target).ok().map(|e| e.id()));

            // A commit "touched" the path if the blob differs from every
            // parent (including the root-commit case, where it's simply new).
            // Using `all` matches git's history simplification: a merge whose
            // result equals either parent's blob is not an editing commit.
            let touched = if commit.parent_count() == 0 {
                blob.is_some()
            } else {
                commit.parents().all(|parent| {
                    let parent_blob = parent
                        .tree()
                        .ok()
                        .and_then(|t| t.get_path(target).ok().map(|e| e.id()));
                    parent_blob != blob
                })
            };
            if touched {
                return Ok(Some(oid.to_string()));
            }
        }
        Ok(None)
    }

    /// Whether Git reports any staged or unstaged non-ignored change.
    /// Hidden paths and non-Markdown files are intentionally included.
    pub fn has_dirty_changes(&self) -> Result<bool> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .context("Failed to get repository status")?;
        Ok(statuses
            .iter()
            .any(|entry| entry.status() != git2::Status::CURRENT))
    }

    /// List paths with unmerged conflict entries in the index.
    pub fn list_conflicted_paths(&self) -> Result<Vec<String>> {
        let mut index = self.repo.index().context("Failed to get index")?;
        index
            .read(true)
            .context("Failed to refresh index before listing conflicts")?;
        if !index.has_conflicts() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in index.conflicts().context("Failed to iterate conflicts")? {
            let conflict = entry.context("Failed to read conflict entry")?;
            let path_bytes = conflict
                .our
                .as_ref()
                .or(conflict.their.as_ref())
                .or(conflict.ancestor.as_ref())
                .map(|e| e.path.clone());
            if let Some(bytes) = path_bytes {
                if let Ok(s) = String::from_utf8(bytes) {
                    if !paths.contains(&s) {
                        paths.push(s);
                    }
                }
            }
        }
        Ok(paths)
    }

    /// Fetch `<remote>/<branch>` and merge it into HEAD. See `merge_fetched`
    /// for behavior. This is the convenience wrapper that does fetch+merge
    /// in one call; callers who fetched separately should use `merge_fetched`.
    pub fn merge_from_remote(&self, remote: &str, branch: &str) -> Result<MergeOutcome> {
        self.fetch(remote, branch)?;
        self.merge_fetched(remote, branch)
    }

    /// Merge an already-fetched `<remote>/<branch>` into HEAD.
    ///
    /// - Up-to-date or fast-forward: returns `MergeOutcome::Clean`.
    /// - 3-way merge with no conflicts: creates a merge commit and returns `Clean`.
    /// - 3-way merge with conflicts: leaves MERGE_HEAD + conflict markers in
    ///   the working tree, returns `Conflicts(paths)`. Caller resolves, stages,
    ///   then calls `finalize_merge_commit` to complete.
    pub fn merge_fetched(&self, remote: &str, branch: &str) -> Result<MergeOutcome> {
        let expected_head = self.capture_head_on_branch(branch)?;
        let upstream_refname = format!("refs/remotes/{}/{}", remote, branch);
        let upstream_oid = self
            .repo
            .refname_to_id(&upstream_refname)
            .with_context(|| format!("Failed to resolve {}", upstream_refname))?;
        let upstream = self
            .repo
            .find_annotated_commit(upstream_oid)
            .context("Failed to load upstream as annotated commit")?;

        let (analysis, _pref) = self
            .repo
            .merge_analysis(&[&upstream])
            .context("Failed to analyze merge")?;

        if analysis.is_up_to_date() {
            self.ensure_head_matches(&expected_head, "finishing merge analysis")?;
            return Ok(MergeOutcome::Clean);
        }

        if analysis.is_fast_forward() {
            self.fast_forward_to(upstream_oid, &expected_head)?;
            return Ok(MergeOutcome::Clean);
        }

        if analysis.is_normal() {
            // True 3-way merge.
            self.ensure_head_matches(&expected_head, "starting merge")?;
            self.repo
                .merge(&[&upstream], None, None)
                .context("Failed to perform merge")?;

            let conflicted = self.list_conflicted_paths()?;
            if !conflicted.is_empty() {
                self.ensure_head_matches(&expected_head, "recording merge conflicts")?;
                return Ok(MergeOutcome::Conflicts(conflicted));
            }

            // Clean merge, write tree and create merge commit.
            self.create_merge_commit_with_expected_head(
                "Merge remote-tracking branch via CommitBook",
                &expected_head,
            )?;
            return Ok(MergeOutcome::Clean);
        }

        bail!(
            "Unexpected merge analysis state: up_to_date={} fast_forward={} normal={} unborn={}",
            analysis.is_up_to_date(),
            analysis.is_fast_forward(),
            analysis.is_normal(),
            analysis.is_unborn()
        )
    }

    /// Complete a merge after the caller has resolved conflicts and staged
    /// the resolved files. Creates the merge commit (HEAD + MERGE_HEAD as
    /// parents) and clears MERGE_HEAD.
    pub fn finalize_merge_commit(&self, message: Option<&str>) -> Result<()> {
        let expected_head = self.capture_head_expectation()?;
        self.finalize_merge_commit_with_expected_head(message, &expected_head)
    }

    /// Complete a merge only while HEAD remains on the configured branch.
    pub fn finalize_merge_commit_on_branch(
        &self,
        message: Option<&str>,
        expected_branch: &str,
    ) -> Result<()> {
        let expected_head = self.capture_head_on_branch(expected_branch)?;
        self.finalize_merge_commit_with_expected_head(message, &expected_head)
    }

    fn finalize_merge_commit_with_expected_head(
        &self,
        message: Option<&str>,
        expected_head: &HeadExpectation,
    ) -> Result<()> {
        if !self.merge_in_progress() {
            bail!("Cannot finalize merge: no merge is in progress");
        }
        let mut index = self.repo.index().context("Failed to get index")?;
        index
            .read(true)
            .context("Failed to refresh index before finalizing merge")?;
        if index.has_conflicts() {
            bail!("Cannot finalize merge: index still has conflicts");
        }
        drop(index);
        let msg = message.unwrap_or("Merge resolved via CommitBook");
        self.create_merge_commit_with_expected_head(msg, expected_head)?;
        Ok(())
    }

    /// True if a merge is in progress (MERGE_HEAD present). A previous
    /// manual-mode or failed-resolver cycle can leave the repo in this state;
    /// the scheduler recovers from it before touching the working tree.
    pub fn merge_in_progress(&self) -> bool {
        self.repo.state() == git2::RepositoryState::Merge
    }

    /// Operation the repository is in the middle of (merge, cherry-pick,
    /// revert, rebase, am, bisect), or `Clean`.
    pub fn repository_state(&self) -> git2::RepositoryState {
        self.repo.state()
    }

    /// True when every `MERGE_HEAD` commit is `refs/remotes/<remote>/<branch>`
    /// or one of its ancestors, i.e. the merge is one sync itself started from
    /// the remote branch rather than a merge the user started.
    pub fn merge_head_is_from_remote(&self, remote: &str, branch: &str) -> Result<bool> {
        let upstream = match self
            .repo
            .refname_to_id(&format!("refs/remotes/{remote}/{branch}"))
        {
            Ok(oid) => oid,
            Err(_) => return Ok(false),
        };
        let path = self.repo.path().join("MERGE_HEAD");
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error).context("Failed to read MERGE_HEAD"),
        };
        let heads = content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(git2::Oid::from_str)
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("MERGE_HEAD does not contain commit ids")?;
        if heads.is_empty() {
            return Ok(false);
        }
        for head in heads {
            if head != upstream && !self.repo.graph_descendant_of(upstream, head)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Abort an in-flight merge: clear MERGE_HEAD and reset working tree
    /// to HEAD. Used in error-recovery paths.
    pub fn merge_abort(&self) -> Result<()> {
        let head = self.repo.head()?.peel_to_commit()?;
        self.repo
            .reset(head.as_object(), git2::ResetType::Hard, None)
            .context("Failed to reset to HEAD during merge abort")?;
        self.repo
            .cleanup_state()
            .context("Failed to cleanup merge state")?;
        Ok(())
    }

    /// Internal: write the index tree and create a commit. If `MERGE_HEAD`
    /// exists, the commit has two parents (HEAD + MERGE_HEAD), and
    /// `MERGE_HEAD` is cleaned up afterwards. Honors `commit.gpgsign`.
    fn create_merge_commit_with_expected_head(
        &self,
        message: &str,
        expected_head: &HeadExpectation,
    ) -> Result<()> {
        let mut index = self.repo.index().context("Failed to get index")?;
        index
            .read(true)
            .context("Failed to refresh index before merge commit")?;
        let tree_oid = index.write_tree().context("Failed to write tree")?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .context("Failed to find tree")?;
        let sig = self
            .repo
            .signature()
            .or_else(|_| Signature::now("CommitBook", "commitbook@localhost"))
            .context("Failed to create signature")?;
        let head_oid = expected_head
            .oid
            .context("Cannot create a merge commit from an unborn HEAD")?;
        let head_commit = self
            .repo
            .find_commit(head_oid)
            .context("Failed to load expected merge parent")?;
        let merge_head_commit = self
            .repo
            .find_reference("MERGE_HEAD")
            .ok()
            .and_then(|r| r.peel_to_commit().ok());

        let parents: Vec<&git2::Commit> = match &merge_head_commit {
            Some(mh) => vec![&head_commit, mh],
            None => vec![&head_commit],
        };

        self.commit_signed_or_plain(&sig, message, &tree, &parents, expected_head)?;

        // Clear MERGE_HEAD if it existed.
        let _ = self.repo.cleanup_state();
        Ok(())
    }
}

enum WorktreeSnapshot {
    Missing,
    File(Vec<u8>, std::fs::Permissions),
    Symlink(PathBuf),
    Directory,
}

impl WorktreeSnapshot {
    fn capture(path: &Path) -> Result<Self> {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(error) => Err(error.into()),
            Ok(meta) if meta.file_type().is_symlink() => {
                Ok(Self::Symlink(std::fs::read_link(path)?))
            }
            Ok(meta) if meta.is_file() => Ok(Self::File(std::fs::read(path)?, meta.permissions())),
            Ok(meta) if meta.is_dir() => Ok(Self::Directory),
            Ok(_) => bail!("Cannot snapshot special file {}", path.display()),
        }
    }

    fn restore(&self, path: &Path) -> Result<()> {
        // Avoid rewriting untouched files, particularly when checkout failed
        // because a parent directory is unwritable.
        let current = Self::capture(path)?;
        match (self, &current) {
            (Self::Missing, Self::Missing) | (Self::Directory, Self::Directory) => return Ok(()),
            (Self::File(bytes, perms), Self::File(now, now_perms))
                if bytes == now && perms == now_perms =>
            {
                return Ok(())
            }
            (Self::Symlink(target), Self::Symlink(now)) if target == now => return Ok(()),
            _ => {}
        }
        match current {
            Self::Missing => {}
            Self::Directory => std::fs::remove_dir(path)?, // never recursively delete user content
            _ => std::fs::remove_file(path)?,
        }
        match self {
            Self::Missing => {}
            Self::Directory => std::fs::create_dir_all(path)?,
            Self::File(bytes, perms) => {
                std::fs::create_dir_all(path.parent().context("Missing parent")?)?;
                std::fs::write(path, bytes)?;
                std::fs::set_permissions(path, perms.clone())?;
            }
            Self::Symlink(target) => {
                #[cfg(unix)]
                {
                    std::fs::create_dir_all(path.parent().context("Missing parent")?)?;
                    std::os::unix::fs::symlink(target, path)?;
                }
                #[cfg(not(unix))]
                bail!(
                    "Cannot restore symlink {} -> {}",
                    path.display(),
                    target.display()
                );
            }
        }
        Ok(())
    }
}

fn restore_index(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("Missing index parent")?)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn validate_selected_path(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("Selected commit path must be a normalized repository-relative path: {path:?}");
    }
    Ok(path.to_path_buf())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

#[cfg(unix)]
fn selected_file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o111 != 0 {
        0o100755
    } else {
        0o100644
    }
}

#[cfg(not(unix))]
fn selected_file_mode(_metadata: &std::fs::Metadata) -> u32 {
    0o100644
}

fn push_rejection_error(message: &str) -> anyhow::Error {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("non-fast-forward")
        || normalized.contains("non fast forward")
        || normalized.contains("fetch first")
    {
        git2::Error::new(
            git2::ErrorCode::NotFastForward,
            git2::ErrorClass::Net,
            format!("Push rejected by remote: {message}"),
        )
        .into()
    } else {
        anyhow::anyhow!("Push rejected by remote: {message}")
    }
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
