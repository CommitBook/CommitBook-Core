//! Bridge from FFI's `sync_commitbook(id, mode, token)` down to engine's
//! `sync_with_resolver`. Runs from `tokio::task::spawn_blocking` since
//! libgit2 calls block.

use std::path::Path;

use commitbook_engine::ai::{ConflictResolver, ResolverRegistry};
use commitbook_engine::commitbooks::registry::find_by_id;
use commitbook_engine::config::LocalConfig;
use commitbook_engine::platform::{Logger, TokenCredentials};
use commitbook_engine::sync::scheduler::sync_with_resolver;

use crate::errors::{CommitBookError, Result};
use crate::types::{SyncMode, SyncResultSummary};

/// In-memory logger that swallows messages, apps can subscribe via
/// callback later if desired. For v1, sync results are returned as
/// `SyncResultSummary` and per-line logs are not threaded through FFI.
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

pub fn sync_one_commitbook(
    workspaces_root: &Path,
    commitbook_id: &str,
    mode: SyncMode,
    token: &str,
) -> Result<SyncResultSummary> {
    let cb = find_by_id(workspaces_root, commitbook_id)
        .map_err(|e| CommitBookError::database(format!("Registry scan: {e}")))?
        .ok_or_else(|| {
            CommitBookError::not_found(format!("CommitBook {commitbook_id} not found"))
        })?;

    let cb_dir = cb.local_path.join(".CommitBook");
    let config = LocalConfig::load(&cb.local_path)
        .map_err(|e| CommitBookError::database(format!("Load config: {e}")))?;

    // Resolver selection: AiResolve uses the configured AI resolver; Manual
    // forces None so conflicts surface as manual_conflicts in the result.
    let registry = ResolverRegistry::new();
    let resolver: Option<&dyn ConflictResolver> = match mode {
        SyncMode::AiResolve => registry.get(&config.conflict.resolver),
        SyncMode::Manual => None,
    };

    // Authenticate git operations with the supplied PAT. Desktop delegates to
    // git's credential helper via SystemCredentials, but a sandboxed mobile
    // app has no helper to reach, so we pass the token explicitly.
    let creds = TokenCredentials::new(token.to_string());

    let logger = NullLogger;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CommitBookError::database(format!("Tokio runtime: {e}")))?;
    let outcome = runtime
        .block_on(sync_with_resolver(
            &cb_dir,
            &cb.local_path,
            &config.git.remote,
            &config.git.branch,
            resolver,
            &creds,
            &logger,
            None,
        ))
        .map_err(|e| CommitBookError::merge(format!("Sync failed: {e}")))?;

    Ok(SyncResultSummary {
        committed: outcome.committed,
        pulled: outcome.pulled,
        pushed: outcome.pushed,
        conflicts_resolved: outcome.conflicts_resolved,
        manual_conflicts: outcome.manual_conflicts,
        errors: outcome.errors,
    })
}
