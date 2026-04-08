use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::domain::workspace::{LocalMode, Provider, Workspace, WorkspaceMode};

pub fn insert(conn: &Connection, ws: &Workspace) -> Result<()> {
    conn.execute(
        "INSERT INTO workspaces (
            id, name, mode, provider, remote_url, owner, repo_name,
            branch, local_mode, local_root, merge_mode,
            sync_interval_seconds, auto_sync, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            ws.id,
            ws.name,
            ws.mode.as_str(),
            ws.provider.as_str(),
            ws.remote_url,
            ws.owner,
            ws.repo_name,
            ws.branch,
            ws.local_mode.as_str(),
            ws.local_root,
            ws.merge_mode,
            ws.sync_interval_seconds,
            ws.auto_sync,
            ws.created_at,
            ws.updated_at,
        ],
    )
    .context("Failed to insert workspace")?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Workspace>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, mode, provider, remote_url, owner, repo_name,
                branch, local_mode, local_root, merge_mode,
                sync_interval_seconds, auto_sync, created_at, updated_at
         FROM workspaces WHERE id = ?1",
    )?;

    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_workspace(row)?)),
        None => Ok(None),
    }
}

pub fn list(conn: &Connection) -> Result<Vec<Workspace>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, mode, provider, remote_url, owner, repo_name,
                branch, local_mode, local_root, merge_mode,
                sync_interval_seconds, auto_sync, created_at, updated_at
         FROM workspaces ORDER BY created_at",
    )?;

    let rows = stmt.query_map([], |row| Ok(row_to_workspace(row).unwrap()))?;
    let mut workspaces = Vec::new();
    for ws in rows {
        workspaces.push(ws?);
    }
    Ok(workspaces)
}

pub fn update(conn: &Connection, ws: &Workspace) -> Result<()> {
    conn.execute(
        "UPDATE workspaces SET
            name = ?2, mode = ?3, provider = ?4, remote_url = ?5,
            owner = ?6, repo_name = ?7, branch = ?8, local_mode = ?9,
            local_root = ?10, merge_mode = ?11, sync_interval_seconds = ?12,
            auto_sync = ?13, updated_at = ?14
         WHERE id = ?1",
        params![
            ws.id,
            ws.name,
            ws.mode.as_str(),
            ws.provider.as_str(),
            ws.remote_url,
            ws.owner,
            ws.repo_name,
            ws.branch,
            ws.local_mode.as_str(),
            ws.local_root,
            ws.merge_mode,
            ws.sync_interval_seconds,
            ws.auto_sync,
            ws.updated_at,
        ],
    )
    .context("Failed to update workspace")?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])
        .context("Failed to delete workspace")?;
    Ok(())
}

fn row_to_workspace(row: &rusqlite::Row) -> Result<Workspace> {
    let mode_str: String = row.get(2)?;
    let provider_str: String = row.get(3)?;
    let local_mode_str: String = row.get(8)?;

    Ok(Workspace {
        id: row.get(0)?,
        name: row.get(1)?,
        mode: WorkspaceMode::from_str(&mode_str)
            .unwrap_or(WorkspaceMode::ExistingLocalRepo),
        provider: Provider::from_str(&provider_str)
            .unwrap_or(Provider::GenericGit),
        remote_url: row.get(4)?,
        owner: row.get(5)?,
        repo_name: row.get(6)?,
        branch: row.get(7)?,
        local_mode: LocalMode::from_str(&local_mode_str)
            .unwrap_or(LocalMode::Sandbox),
        local_root: row.get(9)?,
        merge_mode: row.get(10)?,
        sync_interval_seconds: row.get::<_, i64>(11)? as u64,
        auto_sync: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

#[cfg(test)]
#[path = "workspace_repo_tests.rs"]
mod tests;
