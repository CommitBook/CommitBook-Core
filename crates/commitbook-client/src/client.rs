//! Root client object exposed across the FFI boundary. Implements the 12
//! engine methods (auth, discovery, CommitBook lifecycle, documents, sync,
//! and conflicts) on top of `commitbook-engine`.

use std::path::PathBuf;

use crate::errors::{CommitBookError, Result};
use crate::types::*;

pub struct CommitBookEngineClient {
    /// Reserved for future per-app state (currently unused).
    #[allow(dead_code)]
    pub(crate) db_path: PathBuf,
    pub(crate) workspaces_root: PathBuf,
}

impl CommitBookEngineClient {
    /// UniFFI-exposed constructor. UniFFI wraps the return in `Arc<>` itself.
    pub fn new(db_path: String, workspaces_root: String) -> Result<Self> {
        let db_path = PathBuf::from(db_path);
        let workspaces_root = PathBuf::from(workspaces_root);
        std::fs::create_dir_all(&workspaces_root).map_err(|e| {
            CommitBookError::database(format!(
                "Failed to create workspaces root {}: {e}",
                workspaces_root.display()
            ))
        })?;
        Ok(Self {
            db_path,
            workspaces_root,
        })
    }

    pub async fn validate_pat(&self, token: String) -> Result<Vec<RepoInfo>> {
        // Run on the shared runtime: the UDL poller has no ambient tokio
        // runtime, so reqwest would otherwise panic. Awaiting the JoinHandle
        // needs no runtime of its own.
        crate::runtime::runtime()
            .spawn(async move { crate::auth::fetch_user_repos(&token).await })
            .await
            .map_err(|e| CommitBookError::database(format!("Task join: {e}")))?
    }

    pub async fn discover_commitbooks(&self, token: String) -> Result<Vec<DiscoveredCommitBook>> {
        let workspaces_root = self.workspaces_root.clone();
        // Run the whole discovery (reqwest + tokio::spawn + Semaphore) on the
        // shared runtime; the UDL poller provides none of its own.
        crate::runtime::runtime()
            .spawn(async move {
                let repos = crate::auth::fetch_user_repos(&token).await?;

                // Probe each repo's root for .CommitBook/, with bounded
                // concurrency to avoid hammering GitHub or hitting rate limits.
                // One shared client keeps a single connection pool across probes.
                const CONCURRENCY: usize = 8;
                let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
                let token = std::sync::Arc::new(token);
                let client = std::sync::Arc::new(crate::auth::github_client()?);

                let mut tasks = Vec::with_capacity(repos.len());
                for repo in repos {
                    let semaphore = semaphore.clone();
                    let token = token.clone();
                    let client = client.clone();
                    let workspaces_root = workspaces_root.clone();
                    tasks.push(tokio::spawn(async move {
                        let _permit = semaphore.acquire_owned().await.ok();
                        let has_cb = crate::auth::has_dot_commitbook_with_client(
                            &client,
                            &token,
                            &repo.owner,
                            &repo.name,
                        )
                        .await
                        .unwrap_or(false);
                        let slug =
                            commitbook_engine::commitbooks::slug_for(&repo.owner, &repo.name);
                        let already_local = workspaces_root
                            .join(&slug)
                            .join(".CommitBook")
                            .join("config.toml")
                            .exists();
                        DiscoveredCommitBook {
                            owner: repo.owner,
                            repo: repo.name,
                            default_branch: repo.default_branch,
                            is_private: repo.is_private,
                            has_dot_commitbook: has_cb,
                            already_local,
                        }
                    }));
                }

                let mut out = Vec::with_capacity(tasks.len());
                for t in tasks {
                    // Surface a panicked probe instead of silently truncating.
                    match t.await {
                        Ok(r) => out.push(r),
                        Err(e) => {
                            return Err(CommitBookError::transport(format!(
                                "Discovery task failed: {e}"
                            )))
                        }
                    }
                }
                // Existing CommitBooks first (has_dot_commitbook=true), then the rest.
                out.sort_by(|a, b| {
                    b.has_dot_commitbook
                        .cmp(&a.has_dot_commitbook)
                        .then_with(|| a.owner.cmp(&b.owner))
                        .then_with(|| a.repo.cmp(&b.repo))
                });
                Ok(out)
            })
            .await
            .map_err(|e| CommitBookError::database(format!("Task join: {e}")))?
    }

    pub async fn init_commitbook(
        &self,
        input: CommitBookInput,
        token: String,
    ) -> Result<CommitBookSummary> {
        let workspaces_root = self.workspaces_root.clone();
        // Clone + init are blocking libgit2 calls; run them via spawn_blocking
        // on the shared runtime (the UDL poller has no ambient runtime).
        crate::runtime::runtime()
            .spawn(async move {
                tokio::task::spawn_blocking(move || -> Result<CommitBookSummary> {
                    crate::commitbooks_ops::init_local_commitbook(&workspaces_root, &input, &token)
                })
                .await
                .map_err(|e| CommitBookError::database(format!("Task join: {e}")))?
            })
            .await
            .map_err(|e| CommitBookError::database(format!("Task join: {e}")))?
    }

    pub fn list_commitbooks(&self) -> Result<Vec<CommitBookSummary>> {
        let books = commitbook_engine::commitbooks::scan_workspaces_root(&self.workspaces_root)?;
        Ok(books
            .into_iter()
            .map(|cb| CommitBookSummary {
                id: cb.id,
                owner: cb.owner,
                repo: cb.repo,
                name: cb.name,
                mode: cb.mode,
                provider: cb.provider,
                branch: cb.branch,
                auto_sync: cb.auto_sync,
                doc_count: 0, // computed on demand by list_documents
                conflict_count: 0,
            })
            .collect())
    }

    pub fn get_commitbook(&self, commitbook_id: String) -> Result<CommitBookSummary> {
        let cb = commitbook_engine::commitbooks::registry::find_by_id(
            &self.workspaces_root,
            &commitbook_id,
        )?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {commitbook_id} not found"))
        })?;
        Ok(CommitBookSummary {
            id: cb.id,
            owner: cb.owner,
            repo: cb.repo,
            name: cb.name,
            mode: cb.mode,
            provider: cb.provider,
            branch: cb.branch,
            auto_sync: cb.auto_sync,
            doc_count: 0,
            conflict_count: 0,
        })
    }

    pub fn delete_commitbook(&self, commitbook_id: String) -> Result<()> {
        let cb = commitbook_engine::commitbooks::registry::find_by_id(
            &self.workspaces_root,
            &commitbook_id,
        )?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {commitbook_id} not found"))
        })?;
        // Guard against deleting anything outside the managed workspaces root.
        if !cb.local_path.starts_with(&self.workspaces_root) {
            return Err(CommitBookError::invalid_input(format!(
                "Refusing to delete {}: outside workspaces root",
                cb.local_path.display()
            )));
        }
        std::fs::remove_dir_all(&cb.local_path).map_err(|e| {
            CommitBookError::database(format!(
                "Failed to delete clone at {}: {e}",
                cb.local_path.display()
            ))
        })?;
        Ok(())
    }

    pub fn list_documents(&self, commitbook_id: String) -> Result<Vec<DocumentSummary>> {
        crate::documents::list_documents(&self.workspaces_root, &commitbook_id)
    }

    pub fn read_document(&self, commitbook_id: String, path: String) -> Result<DocumentContent> {
        crate::documents::read_document(&self.workspaces_root, &commitbook_id, &path)
    }

    pub fn save_document(
        &self,
        commitbook_id: String,
        path: String,
        content: String,
    ) -> Result<()> {
        crate::documents::save_document(&self.workspaces_root, &commitbook_id, &path, &content)
    }

    pub async fn sync_commitbook(
        &self,
        commitbook_id: String,
        mode: SyncMode,
        token: String,
    ) -> Result<SyncResultSummary> {
        let workspaces_root = self.workspaces_root.clone();
        // sync_one_commitbook is blocking libgit2 work; run it via
        // spawn_blocking on the shared runtime.
        crate::runtime::runtime()
            .spawn(async move {
                tokio::task::spawn_blocking(move || -> Result<SyncResultSummary> {
                    crate::sync_ops::sync_one_commitbook(
                        &workspaces_root,
                        &commitbook_id,
                        mode,
                        &token,
                    )
                })
                .await
                .map_err(|e| CommitBookError::database(format!("Task join: {e}")))?
            })
            .await
            .map_err(|e| CommitBookError::database(format!("Task join: {e}")))?
    }

    pub fn list_conflicts(&self, commitbook_id: String) -> Result<Vec<ConflictSummary>> {
        crate::conflicts::list_conflicts(&self.workspaces_root, &commitbook_id)
    }

    pub fn resolve_conflict(&self, input: ResolveConflictInput) -> Result<()> {
        crate::conflicts::resolve_conflict(&self.workspaces_root, &input)
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
