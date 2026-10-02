//! Root client object exposed across the FFI boundary. Implements the 12
//! engine methods (auth, discovery, CommitBook lifecycle, documents, sync,
//! and conflicts) on top of `commitbook-engine`.

use std::path::PathBuf;
use std::sync::Arc;

use crate::errors::{CommitBookError, Result};
use crate::types::*;

pub struct CommitBookEngineClient {
    pub(crate) workspaces_root: PathBuf,
    pub(crate) conflict_resolver: Option<Arc<dyn ConflictResolverCallback>>,
    pub(crate) credential_callback: Option<Arc<dyn GitCredentialCallback>>,
}

impl CommitBookEngineClient {
    /// UniFFI-exposed constructor. UniFFI wraps the return in `Arc<>` itself.
    pub fn new(
        workspaces_root: String,
        conflict_resolver: Option<Box<dyn ConflictResolverCallback>>,
        credential_callback: Option<Box<dyn GitCredentialCallback>>,
    ) -> Result<Self> {
        let workspaces_root = PathBuf::from(workspaces_root);
        std::fs::create_dir_all(&workspaces_root).map_err(|e| {
            CommitBookError::storage(format!(
                "Failed to create workspaces root {}: {e}",
                workspaces_root.display()
            ))
        })?;
        let workspaces_root = crate::paths::canonicalize_workspaces_root(&workspaces_root)?;
        Ok(Self {
            workspaces_root,
            conflict_resolver: conflict_resolver.map(Arc::from),
            credential_callback: credential_callback.map(Arc::from),
        })
    }

    pub async fn validate_pat(&self, token: String) -> Result<Vec<RepoInfo>> {
        // Run on the shared runtime: the UDL poller has no ambient tokio
        // runtime, so reqwest would otherwise panic. Awaiting the JoinHandle
        // needs no runtime of its own.
        crate::runtime::runtime()
            .spawn(async move { crate::auth::fetch_user_repos(&token).await })
            .await
            .map_err(|e| CommitBookError::storage(format!("Task join: {e}")))?
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
                    let Ok(identity) =
                        commitbook_engine::git::remote::parse_remote_url(&repo.remote_url)
                    else {
                        continue;
                    };
                    if crate::paths::validate_repo_component("owner", &identity.owner).is_err()
                        || crate::paths::validate_repo_component("repository", &identity.repo)
                            .is_err()
                        || crate::paths::validate_init_input(&repo.remote_url, &repo.default_branch)
                            .is_err()
                    {
                        continue;
                    }
                    let semaphore = semaphore.clone();
                    let token = token.clone();
                    let client = client.clone();
                    let workspaces_root = workspaces_root.clone();
                    tasks.push(tokio::spawn(async move {
                        let _permit = semaphore.acquire_owned().await.ok();
                        let has_cb = crate::auth::has_dot_commitbook_with_client(
                            &client,
                            &token,
                            &identity.owner,
                            &identity.repo,
                        )
                        .await
                        .unwrap_or(false);
                        let already_local =
                            crate::paths::remote_is_local(&workspaces_root, &repo.remote_url);
                        DiscoveredCommitBook {
                            remote_url: repo.remote_url,
                            name: repo.name,
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
                        .then_with(|| a.remote_url.cmp(&b.remote_url))
                });
                Ok(out)
            })
            .await
            .map_err(|e| CommitBookError::storage(format!("Task join: {e}")))?
    }

    pub async fn init_commitbook(&self, input: CommitBookInput) -> Result<CommitBookSummary> {
        crate::paths::validate_init_input(&input.remote_url, &input.branch)?;
        let workspaces_root = self.workspaces_root.clone();
        let credential_callback = self.credential_callback.clone();
        // Clone + init are blocking libgit2 calls; run them via spawn_blocking
        // on the shared runtime (the UDL poller has no ambient runtime).
        crate::runtime::runtime()
            .spawn(async move {
                tokio::task::spawn_blocking(move || -> Result<CommitBookSummary> {
                    crate::commitbooks_ops::init_local_commitbook(
                        &workspaces_root,
                        &input,
                        credential_callback,
                    )
                })
                .await
                .map_err(|e| CommitBookError::storage(format!("Task join: {e}")))?
            })
            .await
            .map_err(|e| CommitBookError::storage(format!("Task join: {e}")))?
    }

    /// Clones that `list_commitbooks` leaves out because their config could
    /// not be loaded, with the reason, so the app can offer a fix.
    pub fn list_broken_commitbooks(&self) -> Result<Vec<BrokenCommitBook>> {
        let scan = commitbook_engine::commitbooks::scan_workspaces(&self.workspaces_root)?;
        Ok(scan
            .broken
            .into_iter()
            .map(|clone| BrokenCommitBook {
                path: clone.path.display().to_string(),
                error: clone.error,
            })
            .collect())
    }

    pub fn list_commitbooks(&self) -> Result<Vec<CommitBookSummary>> {
        let books = commitbook_engine::commitbooks::scan_workspaces_root(&self.workspaces_root)?;
        Ok(books
            .into_iter()
            .map(|cb| CommitBookSummary {
                commitbook_local_id: cb.commitbook_local_id,
                remote_url: cb.remote_url,
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

    /// Register a direct child clone. Does not modify Git history or shared config.
    pub fn register_local_commitbook(&self, relative_path: String) -> Result<CommitBookSummary> {
        let mut components = std::path::Path::new(&relative_path).components();
        if !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
            || relative_path.contains(['/', '\\'])
        {
            return Err(CommitBookError::invalid_input(
                "Expected one workspace directory name",
            ));
        }
        let path = self.workspaces_root.join(relative_path);
        crate::paths::validate_managed_clone(&self.workspaces_root, &path)?;
        let lock = commitbook_engine::state::RepoLock::acquire(&path)
            .map_err(|e| CommitBookError::storage(format!("Lock clone: {e:#}")))?;
        let config = commitbook_engine::config::LocalConfig::load(&path)?;
        commitbook_engine::git::remote::remote_identity(&path, &config.git.remote)?;
        let id = commitbook_engine::commitbooks::identity::ensure_locked(&path, &lock)?;
        self.get_commitbook(id)
    }

    pub fn get_commitbook(&self, commitbook_local_id: String) -> Result<CommitBookSummary> {
        let cb =
            crate::paths::find_managed_commitbook(&self.workspaces_root, &commitbook_local_id)?;
        Ok(CommitBookSummary {
            commitbook_local_id: cb.commitbook_local_id,
            remote_url: cb.remote_url,
            name: cb.name,
            mode: cb.mode,
            provider: cb.provider,
            branch: cb.branch,
            auto_sync: cb.auto_sync,
            doc_count: 0,
            conflict_count: 0,
        })
    }

    /// Delete a clone from this device. Unless `force` is set, a clone with
    /// work that exists only here (uncommitted changes, unpushed commits, or
    /// a merge in progress) is kept and the reason returned, because the
    /// deletion could not be undone from the remote.
    pub fn delete_commitbook(&self, commitbook_local_id: String, force: bool) -> Result<()> {
        let cb =
            crate::paths::find_managed_commitbook(&self.workspaces_root, &commitbook_local_id)?;
        let _lock = commitbook_engine::state::RepoLock::acquire(&cb.local_path)
            .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
        if !force {
            if let Some(reason) = unsaved_work(&cb.local_path, &cb.branch)? {
                return Err(CommitBookError::invalid_input(format!(
                    "Not deleting {commitbook_local_id}: it has {reason}. Sync first, or delete with force to discard it."
                )));
            }
        }
        std::fs::remove_dir_all(&cb.local_path).map_err(|e| {
            CommitBookError::storage(format!(
                "Failed to delete clone at {}: {e}",
                cb.local_path.display()
            ))
        })?;
        Ok(())
    }

    pub fn list_documents(&self, commitbook_local_id: String) -> Result<Vec<DocumentSummary>> {
        crate::documents::list_documents(&self.workspaces_root, &commitbook_local_id)
    }

    pub fn read_document(
        &self,
        commitbook_local_id: String,
        path: String,
    ) -> Result<DocumentContent> {
        crate::documents::read_document(&self.workspaces_root, &commitbook_local_id, &path)
    }

    pub fn save_document(
        &self,
        commitbook_local_id: String,
        path: String,
        content: String,
        expected_revision: Option<String>,
    ) -> Result<()> {
        crate::documents::save_document(
            &self.workspaces_root,
            &commitbook_local_id,
            &path,
            &content,
            expected_revision.as_deref(),
        )
    }

    pub async fn sync_commitbook(
        &self,
        commitbook_local_id: String,
        mode: SyncMode,
    ) -> Result<SyncResultSummary> {
        let workspaces_root = self.workspaces_root.clone();
        let conflict_resolver = self.conflict_resolver.clone();
        let credential_callback = self.credential_callback.clone();
        // sync_one_commitbook is blocking libgit2 work; run it via
        // spawn_blocking on the shared runtime.
        crate::runtime::runtime()
            .spawn(async move {
                tokio::task::spawn_blocking(move || -> Result<SyncResultSummary> {
                    crate::sync_ops::sync_one_commitbook(
                        &workspaces_root,
                        &commitbook_local_id,
                        mode,
                        credential_callback,
                        conflict_resolver,
                    )
                })
                .await
                .map_err(|e| CommitBookError::storage(format!("Task join: {e}")))?
            })
            .await
            .map_err(|e| CommitBookError::storage(format!("Task join: {e}")))?
    }

    pub fn list_conflicts(&self, commitbook_local_id: String) -> Result<Vec<ConflictSummary>> {
        crate::conflicts::list_conflicts(&self.workspaces_root, &commitbook_local_id)
    }

    pub fn resolve_conflict(&self, input: ResolveConflictInput) -> Result<()> {
        crate::conflicts::resolve_conflict(&self.workspaces_root, &input)
    }
}

/// Work that exists only in this clone, described for the user, or `None`
/// when everything is committed and pushed to the configured remote branch.
fn unsaved_work(clone: &std::path::Path, branch: &str) -> Result<Option<String>> {
    let repo = commitbook_engine::git::GitRepo::open(clone)
        .map_err(|error| CommitBookError::storage(format!("Open repo: {error:#}")))?;
    if repo.repository_state() != git2::RepositoryState::Clean {
        return Ok(Some(
            "a merge or other git operation in progress".to_string(),
        ));
    }
    if repo
        .has_dirty_changes()
        .map_err(|error| CommitBookError::storage(format!("Check clone changes: {error:#}")))?
    {
        return Ok(Some("changes that were never committed".to_string()));
    }
    let config = commitbook_engine::config::LocalConfig::load(clone)
        .map_err(|error| CommitBookError::storage(format!("Load clone config: {error:#}")))?;
    let current_branch = repo
        .current_branch()
        .map_err(|error| CommitBookError::storage(format!("Check clone branch: {error:#}")))?;
    if current_branch != branch {
        return Ok(Some(format!(
            "a different branch checked out ({current_branch}, expected {branch})"
        )));
    }
    if repo.rev_parse("HEAD").is_err() {
        return Ok(Some("no readable local HEAD".to_string()));
    };
    let tracking_ref = format!("refs/remotes/{}/{branch}", config.git.remote);
    if repo.rev_parse(&tracking_ref).is_err() {
        return Ok(Some(format!(
            "no local record of the pushed {branch} branch"
        )));
    };
    let (ahead, _) = repo
        .ahead_behind("HEAD", &tracking_ref)
        .map_err(|error| CommitBookError::storage(format!("Check unpushed commits: {error:#}")))?;
    Ok((ahead > 0).then(|| format!("{ahead} commit(s) not pushed yet")))
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
