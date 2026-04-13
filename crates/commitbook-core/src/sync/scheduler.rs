use anyhow::Result;
use std::path::Path;

use crate::domain::transport::RemoteTransport;
use crate::logger::FileLogger;

use super::pipeline;
use super::planner;

/// Run a single sync cycle for a repository.
pub async fn sync_repository(
    commitbook_dir: &Path,
    repo_root: &Path,
    branch: &str,
    transport: &dyn RemoteTransport,
    tracked_patterns: &[String],
    logger: &FileLogger,
) -> Result<pipeline::SyncResult> {
    let plan = planner::create_sync_plan(
        commitbook_dir,
        repo_root,
        branch,
        transport,
        tracked_patterns,
    )
    .await?;

    let result =
        pipeline::execute_sync(&plan, commitbook_dir, repo_root, branch, transport, logger)
            .await?;

    Ok(result)
}
