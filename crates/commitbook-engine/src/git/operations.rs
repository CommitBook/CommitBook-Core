use anyhow::{bail, Context, Result};
use git2::{FetchOptions, PushOptions, RemoteCallbacks, Repository, Signature, StatusOptions};
use std::path::{Path, PathBuf};

use crate::platform::{CredentialProvider, SystemCredentials};

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

/// Markdown-only status view used by the sync planner.
///
/// Same source of truth as `ChangesSummary` (git2 statuses) but filtered to
/// `.md` / `.markdown`, with hidden directories (any path segment starting
/// with `.`) skipped.
#[derive(Debug, Default, Clone)]
pub struct StatusSummary {
    pub modified: Vec<String>,
    pub added: Vec<String>,
    pub deleted: Vec<String>,
}

impl StatusSummary {
    pub fn is_empty(&self) -> bool {
        self.modified.is_empty() && self.added.is_empty() && self.deleted.is_empty()
    }
}

/// Outcome of `GitRepo::merge_from_remote`.
///
/// `Clean` covers up-to-date, fast-forward, and a successful 3-way merge with
/// no conflicts (in which case a merge commit is auto-created). `Conflicts`
/// signals unmerged paths in the index — the caller resolves them, stages,
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

    /// Check if a directory is a git repository.
    pub fn is_repo(path: &Path) -> bool {
        Repository::discover(path).is_ok()
    }

    /// Get a summary of all uncommitted changes.
    pub fn changes_summary(&self) -> Result<ChangesSummary> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .context("Failed to get repository status")?;

        let mut summary = ChangesSummary::default();

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("unknown").to_string();
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

    /// Report markdown-only changes in the working tree vs HEAD.
    ///
    /// Filters the git2 status output to `.md` / `.markdown` and skips any
    /// path under a hidden directory. Planner uses this as the single source
    /// of truth for local dirty detection (modifications, additions, and
    /// deletions all come out of one call).
    pub fn status_markdown(&self) -> Result<StatusSummary> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .context("Failed to get repository status")?;

        let mut out = StatusSummary::default();

        for entry in statuses.iter() {
            let Some(path) = entry.path() else { continue };
            if !is_markdown_path(path) {
                continue;
            }
            let s = entry.status();

            if s.is_wt_deleted() || s.is_index_deleted() {
                out.deleted.push(path.to_string());
            } else if s.is_wt_new() || s.is_index_new() {
                out.added.push(path.to_string());
            } else if s.is_wt_modified() || s.is_index_modified() {
                out.modified.push(path.to_string());
            }
        }

        Ok(out)
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
            Err(_) => None, // No HEAD yet — any staged content counts as change.
        };
        let diff = self
            .repo
            .diff_tree_to_index(head_tree.as_ref(), None, None)
            .context("Failed to diff HEAD to index")?;
        Ok(diff.deltas().count() > 0)
    }

    /// Create a commit with the given message. Honors `commit.gpgsign`.
    pub fn commit(&self, message: &str) -> Result<String> {
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

        let parent_commit = self.repo.head().ok().and_then(|head| {
            head.peel_to_commit().ok()
        });

        let parents: Vec<&git2::Commit> = parent_commit.iter().collect();

        let oid = self.commit_signed_or_plain(&sig, message, &tree, &parents)?;

        // Return short hash
        Ok(oid.to_string()[..7].to_string())
    }

    /// Internal: build a commit object, optionally sign it, and write it
    /// to HEAD. Centralizes the signing decision so `commit` and
    /// `create_merge_commit` share the same path. Fast-paths the unsigned
    /// case to a regular `repo.commit` so the index/HEAD update mechanics
    /// are unchanged.
    fn commit_signed_or_plain(
        &self,
        sig: &Signature,
        message: &str,
        tree: &git2::Tree,
        parents: &[&git2::Commit],
    ) -> Result<git2::Oid> {
        // Unsigned fast path — preserves prior behavior exactly.
        let signing_enabled = self
            .repo
            .config()
            .ok()
            .and_then(|c| c.get_bool("commit.gpgsign").ok())
            .unwrap_or(false);
        if !signing_enabled {
            return self
                .repo
                .commit(Some("HEAD"), sig, sig, message, tree, parents)
                .context("Failed to write commit");
        }

        // Signed path: build buffer, sign, write signed commit, update HEAD.
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
                // a plain commit — better than failing the sync.
                return self
                    .repo
                    .commit(Some("HEAD"), sig, sig, message, tree, parents)
                    .context("Failed to write commit");
            }
        };

        // commit_signed doesn't update HEAD; do it manually. Read the HEAD
        // reference itself (not the peeled commit) so this also works on an
        // unborn branch, where `repo.head()` fails but HEAD still symbolically
        // points at `refs/heads/<branch>`.
        let head = self
            .repo
            .find_reference("HEAD")
            .context("Failed to read HEAD reference")?;
        match head.symbolic_target().map(|s| s.to_string()) {
            Some(branch_ref) => {
                drop(head);
                // Creates the branch ref when unborn, force-updates otherwise.
                self.repo
                    .reference(&branch_ref, oid, true, "commitbook: signed commit")
                    .with_context(|| format!("Failed to point {branch_ref} at signed commit"))?;
            }
            None => {
                // Detached HEAD: move HEAD directly onto the new commit.
                drop(head);
                self.repo
                    .set_head_detached(oid)
                    .context("Failed to update detached HEAD to signed commit")?;
            }
        }

        Ok(oid)
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
        let mut remote = self
            .repo
            .find_remote(remote_name)
            .with_context(|| format!("Remote '{}' not found", remote_name))?;

        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(|url, username_from_url, allowed| {
            creds
                .provide(url, username_from_url, allowed)
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

        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        remote
            .push(&[refspec.as_str()], Some(&mut opts))
            .with_context(|| format!("Failed to push {}/{}", remote_name, branch))?;

        // Drop opts (and thus the callback clone) before taking the value.
        drop(opts);
        if let Some(err) = push_error.borrow_mut().take() {
            bail!("Push rejected by remote: {}", err);
        }
        Ok(())
    }

    /// Check if the repo has any remotes configured.
    pub fn has_remote(&self) -> bool {
        self.repo
            .remotes()
            .map(|r| !r.is_empty())
            .unwrap_or(false)
    }

    /// Get the default remote name (usually "origin").
    pub fn default_remote_name(&self) -> Result<String> {
        let remotes = self.repo.remotes().context("Failed to list remotes")?;
        if remotes.is_empty() {
            bail!("No remotes configured");
        }
        Ok(remotes.get(0).unwrap_or("origin").to_string())
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> Result<String> {
        let head = self.repo.head().context("Failed to get HEAD")?;
        let branch = head
            .shorthand()
            .unwrap_or("main")
            .to_string();
        Ok(branch)
    }

    /// Get the repository path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the diff summary (stat) for AI prompt context, truncated.
    ///
    /// Produces a `git diff --stat HEAD` style summary vs HEAD (or vs an empty
    /// tree for the initial commit). Each changed file appears exactly once
    /// with its total delta — regardless of staging state. Covers working-tree
    /// and index in a single diff so the AI never sees a file twice.
    pub fn diff_summary(&self) -> Result<String> {
        let head_tree = self
            .repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_tree().ok());
        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(head_tree.as_ref(), None)
            .context("Failed to compute diff")?;
        let stats = diff.stats().context("Failed to get diff stats")?;
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
        callbacks.credentials(|url, username_from_url, allowed| {
            creds
                .provide(url, username_from_url, allowed)
                .map_err(|e| git2::Error::from_str(&format!("credential provider failed: {e}")))
        });

        let mut opts = FetchOptions::new();
        opts.remote_callbacks(callbacks);

        remote_obj
            .fetch(&[branch], Some(&mut opts), None)
            .with_context(|| format!("Failed to fetch {}/{}", remote, branch))
    }

    /// Attempt a fast-forward-only merge of the given ref into HEAD.
    ///
    /// Returns `Ok(true)` if HEAD was advanced (or already at the target),
    /// `Ok(false)` if the merge would not be fast-forward. Other failures
    /// bubble up as errors.
    pub fn merge_ff_only(&self, refname: &str) -> Result<bool> {
        let target_oid = self
            .repo
            .revparse_single(refname)
            .with_context(|| format!("Failed to resolve ref '{}'", refname))?
            .id();

        let head_ref = self.repo.head().context("Failed to get HEAD")?;
        let head_oid = head_ref.target().context("HEAD has no target")?;

        if head_oid == target_oid {
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

        let head_name = head_ref.name().context("HEAD is detached")?.to_string();
        drop(head_ref);

        self.repo
            .reference(&head_name, target_oid, true, "commitbook: fast-forward")
            .with_context(|| format!("Failed to update {}", head_name))?;
        self.repo.set_head(&head_name)?;
        self.repo
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .context("Failed to check out fast-forwarded HEAD")?;

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
        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() == Some(git2::ObjectType::Blob) {
                if let Some(name) = entry.name() {
                    let path = if dir.is_empty() {
                        name.to_string()
                    } else {
                        format!("{}{}", dir, name)
                    };
                    files.push(path);
                }
            }
            git2::TreeWalkResult::Ok
        })
        .context("Failed to walk tree")?;

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
        revwalk.push_head().context("Failed to push HEAD to revwalk")?;
        revwalk
            .set_sorting(git2::Sort::TIME)
            .context("Failed to set revwalk sorting")?;

        for oid in revwalk {
            let oid = oid.context("Failed to read revwalk entry")?;
            let commit = self.repo.find_commit(oid).context("Failed to find commit")?;
            let blob = commit
                .tree()
                .ok()
                .and_then(|t| t.get_path(target).ok().map(|e| e.id()));

            // A commit "touched" the path if the blob differs from every
            // parent (including the root-commit case, where it's simply new).
            let touched = if commit.parent_count() == 0 {
                blob.is_some()
            } else {
                commit.parents().any(|parent| {
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

    /// Are there any uncommitted markdown changes in the working tree?
    pub fn has_dirty_markdown(&self) -> Result<bool> {
        Ok(!self.status_markdown()?.is_empty())
    }

    /// List paths with unmerged conflict entries in the index.
    pub fn list_conflicted_paths(&self) -> Result<Vec<String>> {
        let index = self.repo.index().context("Failed to get index")?;
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
            return Ok(MergeOutcome::Clean);
        }

        if analysis.is_fast_forward() {
            // Update HEAD ref + checkout the new tree. Use `force` because
            // by the time we get here the scheduler has already committed
            // any dirty markdown — there's nothing to lose. `safe` mode
            // skips creating files that aren't tracked locally.
            let head_ref = self.repo.head().context("Failed to get HEAD")?;
            let head_name = head_ref
                .name()
                .context("HEAD is detached")?
                .to_string();
            drop(head_ref);
            self.repo
                .reference(&head_name, upstream_oid, true, "commitbook: fast-forward")
                .with_context(|| format!("Failed to update {}", head_name))?;
            self.repo.set_head(&head_name)?;
            self.repo
                .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
                .context("Failed to check out fast-forwarded HEAD")?;
            return Ok(MergeOutcome::Clean);
        }

        if analysis.is_normal() {
            // True 3-way merge.
            self.repo
                .merge(&[&upstream], None, None)
                .context("Failed to perform merge")?;

            let conflicted = self.list_conflicted_paths()?;
            if !conflicted.is_empty() {
                return Ok(MergeOutcome::Conflicts(conflicted));
            }

            // Clean merge — write tree and create merge commit.
            self.create_merge_commit("Merge remote-tracking branch via CommitBook")?;
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
        let index = self.repo.index().context("Failed to get index")?;
        if index.has_conflicts() {
            bail!("Cannot finalize merge: index still has conflicts");
        }
        drop(index);
        let msg = message.unwrap_or("Merge resolved via CommitBook");
        self.create_merge_commit(msg)?;
        Ok(())
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
    fn create_merge_commit(&self, message: &str) -> Result<()> {
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
        let head_commit = self.repo.head()?.peel_to_commit()?;
        let merge_head_commit = self
            .repo
            .find_reference("MERGE_HEAD")
            .ok()
            .and_then(|r| r.peel_to_commit().ok());

        let parents: Vec<&git2::Commit> = match &merge_head_commit {
            Some(mh) => vec![&head_commit, mh],
            None => vec![&head_commit],
        };

        self.commit_signed_or_plain(&sig, message, &tree, &parents)?;

        // Clear MERGE_HEAD if it existed.
        let _ = self.repo.cleanup_state();
        Ok(())
    }
}

fn is_markdown_path(p: &str) -> bool {
    if p.split('/').any(|seg| seg.starts_with('.')) {
        return false;
    }
    let lower = p.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
