use anyhow::{bail, Context, Result};
use git2::{Repository, Signature, StatusOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PUSH_TIMEOUT: Duration = Duration::from_secs(30);

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
        Repository::open(path).is_ok()
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
    pub fn diff_summary(&self) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", "--stat", "--no-color"])
            .current_dir(&self.path)
            .output()
            .context("Failed to get diff summary")?;

        let mut result = String::from_utf8_lossy(&output.stdout).to_string();

        // Also get staged diff
        let staged = Command::new("git")
            .args(["diff", "--staged", "--stat", "--no-color"])
            .current_dir(&self.path)
            .output();

        if let Ok(staged) = staged {
            let staged_text = String::from_utf8_lossy(&staged.stdout);
            if !staged_text.is_empty() {
                result = format!("{}{}", staged_text, result);
            }
        }

        // Truncate to avoid overwhelming AI prompts
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

#[cfg(test)]
fn create_temp_repo() -> (tempfile::TempDir, Repository) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    // Set user config so commits work
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test User").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();

    (tmp, repo)
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
mod tests {
    use super::*;
    use std::fs;

    // --- ChangesSummary tests ---

    #[test]
    fn test_changes_summary_empty() {
        let s = ChangesSummary::default();
        assert!(s.is_empty());
        assert_eq!(s.total(), 0);
    }

    #[test]
    fn test_changes_summary_total() {
        let s = ChangesSummary {
            new_files: vec!["a.rs".into()],
            modified_files: vec!["b.rs".into(), "c.rs".into()],
            deleted_files: vec![],
        };
        assert_eq!(s.total(), 3);
        assert!(!s.is_empty());
    }

    #[test]
    fn test_changes_summary_text_empty() {
        let s = ChangesSummary::default();
        assert_eq!(s.to_summary_text(), "no changes");
    }

    #[test]
    fn test_changes_summary_text_mixed() {
        let s = ChangesSummary {
            new_files: vec!["a.rs".into()],
            modified_files: vec!["b.rs".into()],
            deleted_files: vec!["c.rs".into()],
        };
        let text = s.to_summary_text();
        assert!(text.contains("1 new"));
        assert!(text.contains("1 modified"));
        assert!(text.contains("1 deleted"));
    }

    #[test]
    fn test_changes_detail_text() {
        let s = ChangesSummary {
            new_files: vec!["main.rs".into()],
            modified_files: vec![],
            deleted_files: vec![],
        };
        let detail = s.to_detail_text();
        assert!(detail.contains("main.rs"));
        assert!(detail.contains("1 new file(s)"));
    }

    #[test]
    fn test_changes_detail_text_empty() {
        let s = ChangesSummary::default();
        assert_eq!(s.to_detail_text(), "No changes");
    }

    // --- GitRepo tests ---

    #[test]
    fn test_is_repo_true() {
        let (tmp, _repo) = create_temp_repo();
        assert!(GitRepo::is_repo(tmp.path()));
    }

    #[test]
    fn test_is_repo_false() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!GitRepo::is_repo(tmp.path()));
    }

    #[test]
    fn test_open_repo() {
        let (tmp, _repo) = create_temp_repo();
        let git_repo = GitRepo::open(tmp.path());
        assert!(git_repo.is_ok());
    }

    #[test]
    fn test_has_remote_false() {
        let (tmp, _repo) = create_temp_repo();
        let git_repo = GitRepo::open(tmp.path()).unwrap();
        assert!(!git_repo.has_remote());
    }

    #[test]
    fn test_current_branch_initial() {
        let (tmp, _repo) = create_temp_repo();
        // Need at least one commit for HEAD to exist
        let git_repo = GitRepo::open(tmp.path()).unwrap();
        fs::write(tmp.path().join("init.txt"), "hello").unwrap();
        git_repo.stage_all().unwrap();
        git_repo.commit("initial").unwrap();

        let branch = git_repo.current_branch().unwrap();
        // git init creates "main" or "master" depending on config
        assert!(!branch.is_empty());
    }

    #[test]
    fn test_stage_and_commit_lifecycle() {
        let (tmp, _repo) = create_temp_repo();
        let git_repo = GitRepo::open(tmp.path()).unwrap();

        // Create a file
        fs::write(tmp.path().join("test.txt"), "content").unwrap();

        // Check changes
        let changes = git_repo.changes_summary().unwrap();
        assert!(!changes.is_empty());
        assert_eq!(changes.new_files.len(), 1);

        // Stage all
        git_repo.stage_all().unwrap();

        // Commit
        let hash = git_repo.commit("test commit").unwrap();
        assert_eq!(hash.len(), 7);

        // After commit, no changes
        let changes = git_repo.changes_summary().unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_changes_summary_modified() {
        let (tmp, _repo) = create_temp_repo();
        let git_repo = GitRepo::open(tmp.path()).unwrap();

        // Initial commit
        let file_path = tmp.path().join("file.txt");
        fs::write(&file_path, "v1").unwrap();
        git_repo.stage_all().unwrap();
        git_repo.commit("init").unwrap();

        // Modify file
        fs::write(&file_path, "v2").unwrap();
        let changes = git_repo.changes_summary().unwrap();
        assert_eq!(changes.modified_files.len(), 1);
    }

    #[test]
    fn test_changes_summary_deleted() {
        let (tmp, _repo) = create_temp_repo();
        let git_repo = GitRepo::open(tmp.path()).unwrap();

        // Initial commit
        let file_path = tmp.path().join("doomed.txt");
        fs::write(&file_path, "bye").unwrap();
        git_repo.stage_all().unwrap();
        git_repo.commit("init").unwrap();

        // Delete file
        fs::remove_file(&file_path).unwrap();
        let changes = git_repo.changes_summary().unwrap();
        assert_eq!(changes.deleted_files.len(), 1);
    }
}
