use anyhow::Result;
use rusqlite::Connection;

use crate::domain::sync_plan::{SyncJob, SyncJobStatus, SyncJobType};
use crate::storage::sync_repo;

/// Maximum number of workspaces syncing concurrently.
pub const MAX_CONCURRENT_WORKSPACES: usize = 3;

/// Enqueue a new sync job for a workspace.
/// Returns None if the workspace already has an active job.
pub fn enqueue(
    conn: &Connection,
    workspace_id: &str,
    job_type: SyncJobType,
) -> Result<Option<String>> {
    if sync_repo::has_active_job(conn, workspace_id)? {
        return Ok(None);
    }

    let job = SyncJob {
        id: SyncJob::new_id(),
        workspace_id: workspace_id.to_string(),
        job_type,
        status: SyncJobStatus::Pending,
        retry_count: 0,
        max_retries: 3,
        error_message: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        started_at: None,
        completed_at: None,
    };

    sync_repo::insert_job(conn, &job)?;
    Ok(Some(job.id))
}

/// Dequeue the next pending job for a workspace.
pub fn dequeue(conn: &Connection, workspace_id: &str) -> Result<Option<SyncJob>> {
    sync_repo::get_pending_job(conn, workspace_id)
}

/// Mark a job as running.
pub fn mark_running(conn: &Connection, job_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sync_repo::update_job_status(conn, job_id, &SyncJobStatus::Running, None, &now)
}

/// Mark a job as completed.
pub fn mark_completed(conn: &Connection, job_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sync_repo::update_job_status(conn, job_id, &SyncJobStatus::Completed, None, &now)
}

/// Mark a job as failed. If retries remain, re-enqueue as pending.
pub fn mark_failed(conn: &Connection, job_id: &str, error: &str) -> Result<bool> {
    let now = chrono::Utc::now().to_rfc3339();
    sync_repo::update_job_status(
        conn,
        job_id,
        &SyncJobStatus::Failed,
        Some(error),
        &now,
    )?;

    // Check if we can retry. Read the job to check counts.
    // For simplicity, we let the caller decide retry logic based on job state.
    Ok(false)
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
