//! Bridge from the FFI sync method to the shared engine orchestrator.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result as AnyResult};
use async_trait::async_trait;
use commitbook_engine::ai::{ConflictResolution, ConflictResolver};
use commitbook_engine::config::LocalConfig;
use commitbook_engine::git::GitConflict;
use commitbook_engine::platform::{Logger, TokenCredentials};
use commitbook_engine::state::RepoLock;
use commitbook_engine::sync::scheduler::{sync_with_resolver_locked, SyncOptions};

use crate::errors::{CommitBookError, Result};
use crate::types::{
    AiConflictRequest, AiConflictResolutionAction, ConflictResolutionContinuation,
    ConflictResolverCallback, SyncMode, SyncResultSummary,
};

#[cfg(not(test))]
const HOST_RESOLVER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
#[cfg(test)]
const HOST_RESOLVER_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(150);

struct NullLogger;

impl Logger for NullLogger {
    fn emit(
        &self,
        _level: commitbook_engine::platform::LogLevel,
        _message: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

struct HostConflictResolver {
    commitbook_id: String,
    callback: Arc<dyn ConflictResolverCallback>,
}

#[async_trait]
impl ConflictResolver for HostConflictResolver {
    fn name(&self) -> &str {
        "host app"
    }

    fn key(&self) -> &str {
        "host-app"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn resolve(
        &self,
        conflict: &GitConflict,
        _repo_path: &Path,
    ) -> AnyResult<ConflictResolution> {
        if conflict.is_binary_or_special() {
            bail!(
                "Conflict {} is binary or special and requires manual resolution",
                conflict.path
            );
        }
        let request = AiConflictRequest {
            commitbook_id: self.commitbook_id.clone(),
            path: conflict.path.clone(),
            conflict_type: conflict.classification().to_string(),
            binary: false,
            ancestor_content: conflict.ancestor_text().map(ToOwned::to_owned),
            local_content: conflict.local_text().map(ToOwned::to_owned),
            remote_content: conflict.remote_text().map(ToOwned::to_owned),
        };
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let continuation = Arc::new(ConflictResolutionContinuation::new(sender));
        let callback = Arc::clone(&self.callback);
        let (invocation_sender, invocation_receiver) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("commitbook-host-resolver".to_string())
            .spawn(move || {
                let invocation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    callback.resolve(request, continuation);
                }))
                .map_err(panic_message);
                let _ = invocation_sender.send(invocation);
            })
            .map_err(|error| anyhow::anyhow!("Failed to start host resolver worker: {error}"))?;

        // The same deadline covers both entering foreign callback code and
        // waiting for its continuation. A blocking or panicking Swift/Kotlin
        // implementation therefore cannot retain the repository lock forever.
        let result = tokio::time::timeout(HOST_RESOLVER_TIMEOUT, async {
            invocation_receiver
                .await
                .map_err(|_| anyhow::anyhow!("Host resolver worker exited unexpectedly"))??;
            receiver
                .await
                .map_err(|_| anyhow::anyhow!("Host resolver callback dropped its continuation"))
        })
        .await
        .map_err(|_| anyhow::anyhow!("Host resolver callback timed out"))??;
        let response = match (
            result.resolution,
            result
                .error_message
                .filter(|error| !error.trim().is_empty()),
        ) {
            (Some(resolution), None) => resolution,
            (None, Some(error)) => bail!("Host resolver callback failed: {error}"),
            (Some(_), Some(_)) => bail!("Host resolver returned both a resolution and an error"),
            (None, None) => bail!("Host resolver returned neither a resolution nor an error"),
        };

        match response.action {
            AiConflictResolutionAction::WriteContent => {
                let content = response
                    .content
                    .ok_or_else(|| anyhow::anyhow!("WriteContent requires non-null content"))?;
                if commitbook_engine::git::conflicts::has_conflict_markers(&content) {
                    bail!("Host resolver left conflict markers in {}", conflict.path);
                }
                Ok(ConflictResolution::WriteContent(content))
            }
            AiConflictResolutionAction::DeleteFile => {
                if response.content.is_some() {
                    bail!("DeleteFile must not include content");
                }
                Ok(ConflictResolution::DeleteFile)
            }
        }
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> anyhow::Error {
    let message = payload
        .downcast_ref::<&str>()
        .map(|value| (*value).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic payload".to_string());
    anyhow::anyhow!("Host resolver callback panicked: {message}")
}

pub fn sync_one_commitbook(
    workspaces_root: &Path,
    commitbook_id: &str,
    mode: SyncMode,
    token: &str,
    callback: Option<Arc<dyn ConflictResolverCallback>>,
) -> Result<SyncResultSummary> {
    let commitbook = crate::paths::find_managed_commitbook(workspaces_root, commitbook_id)?;
    let lock = RepoLock::acquire(&commitbook.local_path)
        .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
    let config = LocalConfig::load(&commitbook.local_path)
        .map_err(|error| CommitBookError::database(format!("Load config: {error}")))?;
    let mut options = SyncOptions::from(&config.git);
    options.review_ai_resolutions = config.conflict.review_ai_resolutions;

    let host_resolver = callback.map(|callback| HostConflictResolver {
        commitbook_id: commitbook_id.to_string(),
        callback,
    });
    let requested_ai_without_callback =
        matches!(mode, SyncMode::AiResolve) && host_resolver.is_none();
    let resolver: Option<&dyn ConflictResolver> = match mode {
        SyncMode::AiResolve => host_resolver
            .as_ref()
            .map(|resolver| resolver as &dyn ConflictResolver),
        SyncMode::Manual => None,
    };

    let creds = TokenCredentials::new(token.to_string());
    let logger = NullLogger;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CommitBookError::database(format!("Tokio runtime: {error}")))?;
    let mut outcome = runtime
        .block_on(sync_with_resolver_locked(
            &commitbook.local_path,
            &options,
            resolver,
            &creds,
            &logger,
            None,
            &lock,
        ))
        .map_err(|error| CommitBookError::merge(format!("Sync failed: {error}")))?;

    if requested_ai_without_callback && outcome.manual_conflicts > 0 {
        outcome.errors.push(
            "AI resolution was requested, but the host app did not register a conflict resolver; configure AI or resolve the conflicts manually"
                .to_string(),
        );
    }

    Ok(SyncResultSummary {
        committed: outcome.committed,
        pulled: outcome.pulled,
        pushed: outcome.pushed,
        conflicts_resolved: outcome.conflicts_resolved,
        manual_conflicts: outcome.manual_conflicts,
        errors: outcome.errors,
    })
}

#[cfg(test)]
#[path = "sync_ops_tests.rs"]
mod tests;
