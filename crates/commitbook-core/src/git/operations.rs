use anyhow::{bail, Context, Result};
use git2::{Repository, Signature, StatusOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PUSH_TIMEOUT: Duration = Duration::from_secs(30);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

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
        let output = Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(&self.path)
            .output()
            .context("Failed to check staged changes")?;

        // exit code 1 = there are differences, 0 = no differences
        Ok(!output.status.success())
    }

    /// Create a commit with the given message.
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

        let oid = self
            .repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .context("Failed to create commit")?;

        // Return short hash
        Ok(oid.to_string()[..7].to_string())
    }

    /// Push to the remote using the git CLI with timeout.
    pub fn push(&self, remote_name: &str, branch: &str) -> Result<()> {
        let child = Command::new("git")
            .args(["push", remote_name, branch])
            .current_dir(&self.path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start git push")?;

        let output = wait_with_timeout(child, PUSH_TIMEOUT)
            .context("git push timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git push failed: {}", stderr.trim());
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
    /// Uses `git diff --stat HEAD` so each changed file appears exactly once
    /// with its total delta vs HEAD — regardless of staging state. This avoids
    /// the AI seeing the same file twice (once staged, once unstaged) when the
    /// working tree changes between `stage_all()` and this call.
    pub fn diff_summary(&self) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--stat", "HEAD", "--no-color"])
            .current_dir(&self.path)
            .output()
            .context("Failed to get diff summary")?;

        let result = if output.status.success() {
            String::from_utf8_lossy(&output.stdout).to_string()
        } else {
            // No HEAD yet (initial commit) — fall back to staged diff.
            let staged = Command::new("git")
                .args(["diff", "--staged", "--stat", "--no-color"])
                .current_dir(&self.path)
                .output()
                .context("Failed to get staged diff summary")?;
            String::from_utf8_lossy(&staged.stdout).to_string()
        };

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

    /// Check if remote has diverged (has commits we don't have).
    pub fn remote_has_diverged(&self, remote: &str, branch: &str) -> Result<bool> {
        // Fetch latest refs
        let _ = Command::new("git")
            .args(["fetch", remote, "--quiet"])
            .current_dir(&self.path)
            .output();

        let output = Command::new("git")
            .args([
                "rev-list",
                "--count",
                &format!("HEAD..{}/{}", remote, branch),
            ])
            .current_dir(&self.path)
            .output()
            .context("Failed to check for diverged remote")?;

        if !output.status.success() {
            return Ok(false); // Can't determine, assume not diverged
        }

        let count: usize = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        Ok(count > 0)
    }

    /// Fetch a specific branch from the remote.
    pub fn fetch(&self, remote: &str, branch: &str) -> Result<()> {
        let child = Command::new("git")
            .args(["fetch", remote, branch])
            .current_dir(&self.path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to start git fetch")?;

        let output = wait_with_timeout(child, FETCH_TIMEOUT)
            .context("git fetch timed out")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git fetch failed: {}", stderr.trim());
        }

        Ok(())
    }

    /// Attempt a fast-forward-only merge of the given ref into HEAD.
    ///
    /// Returns `Ok(true)` if HEAD was advanced (or already at the target),
    /// `Ok(false)` if the merge would not be fast-forward. Other failures
    /// bubble up as errors.
    pub fn merge_ff_only(&self, refname: &str) -> Result<bool> {
        let ancestor = Command::new("git")
            .args(["merge-base", "--is-ancestor", "HEAD", refname])
            .current_dir(&self.path)
            .output()
            .context("Failed to run git merge-base")?;

        // Exit 0 = HEAD is an ancestor of refname (including equal).
        // Exit 1 = not an ancestor. Other codes are real errors.
        match ancestor.status.code() {
            Some(0) => {}
            Some(1) => return Ok(false),
            _ => {
                let stderr = String::from_utf8_lossy(&ancestor.stderr);
                bail!("git merge-base failed: {}", stderr.trim());
            }
        }

        let output = Command::new("git")
            .args(["merge", "--ff-only", refname])
            .current_dir(&self.path)
            .output()
            .context("Failed to run git merge --ff-only")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git merge --ff-only failed: {}", stderr.trim());
        }

        Ok(true)
    }

    /// Count commits ahead and behind between two refs.
    ///
    /// Returns `(ahead, behind)`: how many commits `local` has that `remote` doesn't
    /// (ahead) and vice versa (behind).
    pub fn ahead_behind(&self, local: &str, remote: &str) -> Result<(u32, u32)> {
        let output = Command::new("git")
            .args([
                "rev-list",
                "--left-right",
                "--count",
                &format!("{local}...{remote}"),
            ])
            .current_dir(&self.path)
            .output()
            .context("Failed to run git rev-list")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git rev-list failed: {}", stderr.trim());
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut parts = text.split_whitespace();
        let ahead: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let behind: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Ok((ahead, behind))
    }

    /// Read a file's content at a specific ref (commit/branch/tag).
    pub fn show_file_at_ref(&self, refname: &str, path: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["show", &format!("{refname}:{path}")])
            .current_dir(&self.path)
            .output()
            .context("Failed to run git show")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git show {}:{} failed: {}", refname, path, stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// List all file paths reachable from a ref.
    pub fn ls_tree_files(&self, refname: &str) -> Result<Vec<String>> {
        let output = Command::new("git")
            .args(["ls-tree", "-r", "--name-only", refname])
            .current_dir(&self.path)
            .output()
            .context("Failed to run git ls-tree")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git ls-tree failed: {}", stderr.trim());
        }

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.lines().map(|s| s.to_string()).collect())
    }

    /// Resolve a ref to its full SHA.
    pub fn rev_parse(&self, refname: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", refname])
            .current_dir(&self.path)
            .output()
            .context("Failed to run git rev-parse")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git rev-parse {} failed: {}", refname, stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Rebase the current branch onto the given ref.
    ///
    /// On conflict, aborts the rebase and returns an error.
    pub fn rebase_onto(&self, refname: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["rebase", refname])
            .current_dir(&self.path)
            .output()
            .context("Failed to run git rebase")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(&self.path)
                .output();
            bail!("git rebase {} failed (aborted): {}", refname, stderr.trim());
        }

        Ok(())
    }

    /// Attempt to pull with rebase to sync with remote.
    pub fn pull_rebase(&self, remote: &str, branch: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["pull", "--rebase", remote, branch])
            .current_dir(&self.path)
            .output()
            .context("Failed to pull --rebase")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Abort the rebase if it failed
            let _ = Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(&self.path)
                .output();
            bail!("pull --rebase failed (aborted): {}", stderr.trim());
        }

        Ok(())
    }
}

/// Wait for a child process with a timeout.
fn wait_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    let mut child = child;
    let start = std::time::Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().context("Failed to get output"),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    bail!("Process timed out after {:?}", timeout);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => bail!("Error waiting for process: {}", e),
        }
    }
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
