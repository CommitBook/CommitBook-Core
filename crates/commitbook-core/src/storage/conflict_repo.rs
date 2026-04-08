use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::domain::conflict::{Conflict, ConflictStatus, ConflictType, ResolutionType};

pub fn insert(conn: &Connection, conflict: &Conflict) -> Result<()> {
    conn.execute(
        "INSERT INTO conflicts (
            id, workspace_id, path, section_path, conflict_type,
            base_content, local_content, remote_content, merged_preview,
            status, resolution_type, opened_at, resolved_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            conflict.id,
            conflict.workspace_id,
            conflict.path,
            conflict.section_path,
            conflict.conflict_type.as_str(),
            conflict.base_content,
            conflict.local_content,
            conflict.remote_content,
            conflict.merged_preview,
            conflict.status.as_str(),
            conflict.resolution_type.as_ref().map(|r| r.as_str()),
            conflict.opened_at,
            conflict.resolved_at,
        ],
    )
    .context("Failed to insert conflict")?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<Conflict>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, path, section_path, conflict_type,
                base_content, local_content, remote_content, merged_preview,
                status, resolution_type, opened_at, resolved_at
         FROM conflicts WHERE id = ?1",
    )?;
    let mut rows = stmt.query(params![id])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_conflict(row)?)),
        None => Ok(None),
    }
}

pub fn list_open(conn: &Connection, workspace_id: &str) -> Result<Vec<Conflict>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, path, section_path, conflict_type,
                base_content, local_content, remote_content, merged_preview,
                status, resolution_type, opened_at, resolved_at
         FROM conflicts WHERE workspace_id = ?1 AND status = 'open'
         ORDER BY opened_at",
    )?;
    let rows = stmt.query_map(params![workspace_id], |row| {
        Ok(row_to_conflict(row).unwrap())
    })?;
    let mut conflicts = Vec::new();
    for c in rows {
        conflicts.push(c?);
    }
    Ok(conflicts)
}

pub fn resolve(
    conn: &Connection,
    id: &str,
    resolution_type: &ResolutionType,
    resolved_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE conflicts SET
            status = 'resolved',
            resolution_type = ?2,
            resolved_at = ?3
         WHERE id = ?1",
        params![id, resolution_type.as_str(), resolved_at],
    )
    .context("Failed to resolve conflict")?;
    Ok(())
}

pub fn delete_by_workspace(conn: &Connection, workspace_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM conflicts WHERE workspace_id = ?1",
        params![workspace_id],
    )?;
    Ok(())
}

fn row_to_conflict(row: &rusqlite::Row) -> Result<Conflict> {
    let conflict_type_str: String = row.get(4)?;
    let status_str: String = row.get(9)?;
    let resolution_type_str: Option<String> = row.get(10)?;

    Ok(Conflict {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        path: row.get(2)?,
        section_path: row.get(3)?,
        conflict_type: ConflictType::from_str(&conflict_type_str)
            .unwrap_or(ConflictType::FileConflict),
        base_content: row.get(5)?,
        local_content: row.get(6)?,
        remote_content: row.get(7)?,
        merged_preview: row.get(8)?,
        status: ConflictStatus::from_str(&status_str)
            .unwrap_or(ConflictStatus::Open),
        resolution_type: resolution_type_str
            .and_then(|s| ResolutionType::from_str(&s)),
        opened_at: row.get(11)?,
        resolved_at: row.get(12)?,
    })
}
