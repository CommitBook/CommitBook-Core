use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::domain::sync_plan::{SyncCheckpoint, SyncJob, SyncJobStatus, SyncJobType};

// --- Sync Checkpoints ---

pub fn upsert_checkpoint(conn: &Connection, cp: &SyncCheckpoint) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_checkpoints (workspace_id, remote_head, last_completed_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT (workspace_id) DO UPDATE SET
            remote_head = excluded.remote_head,
            last_completed_at = excluded.last_completed_at",
        params![cp.workspace_id, cp.remote_head, cp.last_completed_at],
    )
    .context("Failed to upsert sync checkpoint")?;
    Ok(())
}

pub fn get_checkpoint(conn: &Connection, workspace_id: &str) -> Result<Option<SyncCheckpoint>> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, remote_head, last_completed_at
         FROM sync_checkpoints WHERE workspace_id = ?1",
    )?;
    let mut rows = stmt.query(params![workspace_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(SyncCheckpoint {
            workspace_id: row.get(0)?,
            remote_head: row.get(1)?,
            last_completed_at: row.get(2)?,
        })),
        None => Ok(None),
    }
}

// --- Sync Jobs ---

pub fn insert_job(conn: &Connection, job: &SyncJob) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_jobs (
            id, workspace_id, job_type, status, retry_count,
            max_retries, error_message, created_at, started_at, completed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            job.id,
            job.workspace_id,
            job.job_type.as_str(),
            job.status.as_str(),
            job.retry_count,
            job.max_retries,
            job.error_message,
            job.created_at,
            job.started_at,
            job.completed_at,
        ],
    )
    .context("Failed to insert sync job")?;
    Ok(())
}

pub fn get_pending_job(conn: &Connection, workspace_id: &str) -> Result<Option<SyncJob>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, job_type, status, retry_count,
                max_retries, error_message, created_at, started_at, completed_at
         FROM sync_jobs
         WHERE workspace_id = ?1 AND status = 'pending'
         ORDER BY created_at ASC LIMIT 1",
    )?;
    let mut rows = stmt.query(params![workspace_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_job(row)?)),
        None => Ok(None),
    }
}

pub fn update_job_status(
    conn: &Connection,
    job_id: &str,
    status: &SyncJobStatus,
    error_message: Option<&str>,
    timestamp: &str,
) -> Result<()> {
    let (started_update, completed_update) = match status {
        SyncJobStatus::Running => ("started_at = ?4", "completed_at = completed_at"),
        SyncJobStatus::Completed | SyncJobStatus::Failed | SyncJobStatus::Cancelled => {
            ("started_at = started_at", "completed_at = ?4")
        }
        _ => ("started_at = started_at", "completed_at = completed_at"),
    };

    let sql = format!(
        "UPDATE sync_jobs SET status = ?2, error_message = ?3, {started_update}, {completed_update} WHERE id = ?1"
    );
    conn.execute(
        &sql,
        params![job_id, status.as_str(), error_message, timestamp],
    )
    .context("Failed to update sync job status")?;
    Ok(())
}

pub fn increment_retry(conn: &Connection, job_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE sync_jobs SET
            retry_count = retry_count + 1,
            status = 'pending'
         WHERE id = ?1",
        params![job_id],
    )?;
    Ok(())
}

pub fn has_active_job(conn: &Connection, workspace_id: &str) -> Result<bool> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM sync_jobs
         WHERE workspace_id = ?1 AND status IN ('pending', 'running')",
        params![workspace_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn row_to_job(row: &rusqlite::Row) -> Result<SyncJob> {
    let job_type_str: String = row.get(2)?;
    let status_str: String = row.get(3)?;

    Ok(SyncJob {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        job_type: SyncJobType::from_str(&job_type_str)
            .unwrap_or(SyncJobType::SyncNow),
        status: SyncJobStatus::from_str(&status_str)
            .unwrap_or(SyncJobStatus::Pending),
        retry_count: row.get(4)?,
        max_retries: row.get(5)?,
        error_message: row.get(6)?,
        created_at: row.get(7)?,
        started_at: row.get(8)?,
        completed_at: row.get(9)?,
    })
}

// --- Sync Events ---

pub fn log_event(
    conn: &Connection,
    workspace_id: &str,
    job_id: Option<&str>,
    category: &str,
    action: &str,
    result: &str,
    duration_ms: Option<i64>,
    error_code: Option<&str>,
    payload_json: Option<&str>,
) -> Result<()> {
    let id = format!("se_{}", nanoid::nanoid!(12));
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sync_events (
            id, workspace_id, job_id, category, action, result,
            duration_ms, error_code, payload_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            workspace_id,
            job_id,
            category,
            action,
            result,
            duration_ms,
            error_code,
            payload_json,
            now,
        ],
    )?;
    Ok(())
}
