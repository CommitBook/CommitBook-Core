use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;

use crate::domain::transport::{
    RemoteDocument, RemoteTransport, RepoDescriptor, WriteFileInput, WriteFileResult,
};
use crate::git::commit::commit_files;
use crate::git::GitRepo;

/// Transport for a git repo with a remote, operating directly on the user's
/// working copy (no shadow clone).
///
/// Reads remote state from the fetched remote-tracking ref (`<remote>/<branch>`),
/// not from the working tree — so uncommitted local edits never leak into the
/// "remote view" the pipeline reasons about. Writes stage + commit in the user's
/// repo and push to the remote.
pub struct GitRemoteTransport {
    repo_path: PathBuf,
    remote_name: String,
    branch: String,
}

impl GitRemoteTransport {
    pub fn new(repo_path: PathBuf, remote_name: String, branch: String) -> Self {
        Self {
            repo_path,
            remote_name,
            branch,
        }
    }

    fn repo(&self) -> Result<GitRepo> {
        GitRepo::open(&self.repo_path)
    }

    fn remote_ref(&self) -> String {
        format!("{}/{}", self.remote_name, self.branch)
    }
}

#[async_trait]
impl RemoteTransport for GitRemoteTransport {
    async fn validate(&self) -> Result<()> {
        let repo = self.repo()?;
        repo.fetch(&self.remote_name, &self.branch)
    }

    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>> {
        let name = self
            .repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());
        Ok(vec![RepoDescriptor {
            id: self.repo_path.to_string_lossy().to_string(),
            provider: "git".to_string(),
            owner: String::new(),
            name,
            default_branch: self.branch.clone(),
            private: false,
        }])
    }

    async fn list_files(&self, _branch: &str) -> Result<Vec<String>> {
        let repo = self.repo()?;
        repo.fetch(&self.remote_name, &self.branch)?;
        let all = repo.ls_tree_files(&self.remote_ref())?;
        let mut files: Vec<String> = all
            .into_iter()
            .filter(|p| {
                // Match collect_markdown_files in local_repo.rs: .md / .markdown,
                // skip hidden directories.
                if p.split('/').any(|seg| seg.starts_with('.')) {
                    return false;
                }
                let lower = p.to_lowercase();
                lower.ends_with(".md") || lower.ends_with(".markdown")
            })
            .collect();
        files.sort();
        Ok(files)
    }

    async fn read_file(&self, _branch: &str, path: &str) -> Result<RemoteDocument> {
        let repo = self.repo()?;
        let content = repo
            .show_file_at_ref(&self.remote_ref(), path)
            .with_context(|| format!("Failed to read {path} at {}", self.remote_ref()))?;
        let revision = repo.rev_parse(&self.remote_ref())?;
        Ok(RemoteDocument {
            path: path.to_string(),
            content,
            revision,
        })
    }

    async fn write_files(
        &self,
        _branch: &str,
        inputs: Vec<WriteFileInput>,
    ) -> Result<Vec<WriteFileResult>> {
        let results = commit_files(&self.repo_path, inputs)?;
        if results.is_empty() {
            return Ok(results);
        }

        let repo = self.repo()?;
        match repo.push(&self.remote_name, &self.branch) {
            Ok(()) => Ok(results),
            Err(first_err) => {
                // Push may have been rejected because the remote advanced between
                // our planner's fetch and our push. Refetch, try to reconcile, and
                // retry once.
                repo.fetch(&self.remote_name, &self.branch)?;
                let remote_ref = self.remote_ref();
                if !repo.merge_ff_only(&remote_ref)? {
                    repo.rebase_onto(&remote_ref)?;
                }
                repo.push(&self.remote_name, &self.branch)
                    .with_context(|| {
                        format!(
                            "git push retried after fetch/merge and still failed; \
                             original error: {first_err}"
                        )
                    })?;

                // The merge/rebase may have moved HEAD — recompute the revision.
                let new_rev = repo.rev_parse("HEAD")?;
                let updated = results
                    .into_iter()
                    .map(|r| WriteFileResult {
                        path: r.path,
                        new_revision: new_rev.clone(),
                    })
                    .collect();
                Ok(updated)
            }
        }
    }

    async fn delete_file(&self, _branch: &str, path: &str, message: &str) -> Result<()> {
        let full = self.repo_path.join(path);
        if full.exists() {
            std::fs::remove_file(&full)?;
        }

        // Stage the deletion and commit via git CLI (simplest path that handles
        // both stage-delete and empty-tree edge cases identically to local_repo).
        let rm = std::process::Command::new("git")
            .args(["rm", "--ignore-unmatch", path])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to run git rm")?;
        if !rm.status.success() {
            let stderr = String::from_utf8_lossy(&rm.stderr);
            anyhow::bail!("git rm failed: {}", stderr.trim());
        }

        let commit = std::process::Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.repo_path)
            .output()
            .context("Failed to run git commit")?;
        if !commit.status.success() {
            let stderr = String::from_utf8_lossy(&commit.stderr);
            anyhow::bail!("git commit failed: {}", stderr.trim());
        }

        let repo = self.repo()?;
        repo.push(&self.remote_name, &self.branch)?;
        Ok(())
    }

    async fn get_head(&self, _branch: &str) -> Result<String> {
        let repo = self.repo()?;
        repo.fetch(&self.remote_name, &self.branch)?;
        repo.rev_parse(&self.remote_ref())
    }
}

#[cfg(test)]
#[path = "git_remote_tests.rs"]
mod tests;
