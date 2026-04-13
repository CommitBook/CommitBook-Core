use anyhow::{Context, Result};
use async_trait::async_trait;
use git2::{Repository, Signature};
use std::path::{Path, PathBuf};

use crate::domain::transport::{
    RemoteDocument, RemoteTransport, RepoDescriptor, WriteFileInput, WriteFileResult,
};

/// Transport for existing local git repositories on disk.
///
/// Uses git2 for all operations. Reads files directly from the working tree.
/// Writes via staging + commit with git2.
pub struct LocalRepoTransport {
    repo_path: PathBuf,
    branch: String,
}

impl LocalRepoTransport {
    pub fn new(repo_path: PathBuf, branch: String) -> Self {
        Self { repo_path, branch }
    }

    fn open_repo(&self) -> Result<Repository> {
        Repository::open(&self.repo_path)
            .with_context(|| format!("Failed to open repo at {}", self.repo_path.display()))
    }

    fn head_commit_sha(&self, repo: &Repository) -> Result<String> {
        let head = repo.head().context("Failed to get HEAD")?;
        let commit = head.peel_to_commit().context("Failed to peel to commit")?;
        Ok(commit.id().to_string())
    }
}

#[async_trait]
impl RemoteTransport for LocalRepoTransport {
    async fn validate(&self) -> Result<()> {
        let repo = self.open_repo()?;
        // Check the branch exists.
        repo.find_branch(&self.branch, git2::BranchType::Local)
            .with_context(|| format!("Branch '{}' not found", self.branch))?;
        Ok(())
    }

    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>> {
        // Local repo transport only manages one repo.
        let name = self
            .repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "local".to_string());
        Ok(vec![RepoDescriptor {
            id: self.repo_path.to_string_lossy().to_string(),
            provider: "local".to_string(),
            owner: String::new(),
            name,
            default_branch: self.branch.clone(),
            private: false,
        }])
    }

    async fn list_files(&self, _branch: &str) -> Result<Vec<String>> {
        let mut files = Vec::new();
        collect_markdown_files(&self.repo_path, &self.repo_path, &mut files)?;
        files.sort();
        Ok(files)
    }

    async fn read_file(&self, _branch: &str, path: &str) -> Result<RemoteDocument> {
        let full_path = self.repo_path.join(path);
        let content = std::fs::read_to_string(&full_path)
            .with_context(|| format!("Failed to read {}", full_path.display()))?;
        let repo = self.open_repo()?;
        let revision = self.head_commit_sha(&repo).unwrap_or_default();
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
        let repo = self.open_repo()?;

        // Write files to disk.
        for input in &inputs {
            let full_path = self.repo_path.join(&input.path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full_path, &input.content)
                .with_context(|| format!("Failed to write {}", full_path.display()))?;
        }

        // Stage all written files.
        let mut index = repo.index()?;
        for input in &inputs {
            index.add_path(Path::new(&input.path))?;
        }
        index.write()?;

        // Create commit.
        let message = if inputs.len() == 1 {
            inputs[0].message.clone()
        } else {
            format!("Update {} files", inputs.len())
        };

        let tree_oid = index.write_tree()?;
        let parent = repo.head()?.peel_to_commit()?;

        // Skip commit if tree is unchanged from parent.
        if tree_oid == parent.tree_id() {
            log::info!("Tree unchanged; skipping empty commit");
            return Ok(vec![]);
        }

        let tree = repo.find_tree(tree_oid)?;
        let sig = repo
            .signature()
            .unwrap_or_else(|_| Signature::now("CommitBook", "commitbook@local").unwrap());

        let commit_oid = repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &message,
            &tree,
            &[&parent],
        )?;

        let new_revision = commit_oid.to_string();
        let results = inputs
            .iter()
            .map(|input| WriteFileResult {
                path: input.path.clone(),
                new_revision: new_revision.clone(),
            })
            .collect();

        Ok(results)
    }

    async fn delete_file(&self, _branch: &str, path: &str, message: &str) -> Result<()> {
        let full_path = self.repo_path.join(path);
        if full_path.exists() {
            std::fs::remove_file(&full_path)?;
        }

        let repo = self.open_repo()?;
        let mut index = repo.index()?;
        index.remove_path(Path::new(path))?;
        index.write()?;

        let tree_oid = index.write_tree()?;
        let tree = repo.find_tree(tree_oid)?;
        let sig = repo
            .signature()
            .unwrap_or_else(|_| Signature::now("CommitBook", "commitbook@local").unwrap());
        let parent = repo.head()?.peel_to_commit()?;

        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;
        Ok(())
    }

    async fn get_head(&self, _branch: &str) -> Result<String> {
        let repo = self.open_repo()?;
        self.head_commit_sha(&repo)
    }
}

/// Recursively collect markdown files (.md, .markdown) relative to the repo root.
fn collect_markdown_files(root: &Path, dir: &Path, files: &mut Vec<String>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip hidden directories and .git.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }

        if path.is_dir() {
            collect_markdown_files(root, &path, files)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "md" || ext == "markdown" {
                if let Ok(relative) = path.strip_prefix(root) {
                    files.push(relative.to_string_lossy().to_string());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "local_repo_tests.rs"]
mod tests;
