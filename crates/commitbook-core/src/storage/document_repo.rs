use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::domain::document::Document;
use crate::domain::section::Section;

// --- Document CRUD ---

pub fn upsert(conn: &Connection, doc: &Document) -> Result<()> {
    conn.execute(
        "INSERT INTO documents (
            workspace_id, path, local_revision, remote_revision,
            checksum, dirty, deleted, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT (workspace_id, path) DO UPDATE SET
            local_revision = excluded.local_revision,
            remote_revision = excluded.remote_revision,
            checksum = excluded.checksum,
            dirty = excluded.dirty,
            deleted = excluded.deleted,
            updated_at = excluded.updated_at",
        params![
            doc.workspace_id,
            doc.path,
            doc.local_revision,
            doc.remote_revision,
            doc.checksum,
            doc.dirty,
            doc.deleted,
            doc.updated_at,
        ],
    )
    .context("Failed to upsert document")?;
    Ok(())
}

pub fn get(conn: &Connection, workspace_id: &str, path: &str) -> Result<Option<Document>> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, path, local_revision, remote_revision,
                checksum, dirty, deleted, updated_at
         FROM documents WHERE workspace_id = ?1 AND path = ?2",
    )?;

    let mut rows = stmt.query(params![workspace_id, path])?;
    match rows.next()? {
        Some(row) => Ok(Some(row_to_document(row)?)),
        None => Ok(None),
    }
}

pub fn list_by_workspace(conn: &Connection, workspace_id: &str) -> Result<Vec<Document>> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, path, local_revision, remote_revision,
                checksum, dirty, deleted, updated_at
         FROM documents WHERE workspace_id = ?1 ORDER BY path",
    )?;

    let rows = stmt.query_map(params![workspace_id], |row| {
        Ok(row_to_document(row).unwrap())
    })?;
    let mut docs = Vec::new();
    for doc in rows {
        docs.push(doc?);
    }
    Ok(docs)
}

pub fn list_dirty(conn: &Connection, workspace_id: &str) -> Result<Vec<Document>> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, path, local_revision, remote_revision,
                checksum, dirty, deleted, updated_at
         FROM documents WHERE workspace_id = ?1 AND dirty = 1 ORDER BY path",
    )?;

    let rows = stmt.query_map(params![workspace_id], |row| {
        Ok(row_to_document(row).unwrap())
    })?;
    let mut docs = Vec::new();
    for doc in rows {
        docs.push(doc?);
    }
    Ok(docs)
}

pub fn delete(conn: &Connection, workspace_id: &str, path: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM documents WHERE workspace_id = ?1 AND path = ?2",
        params![workspace_id, path],
    )
    .context("Failed to delete document")?;
    Ok(())
}

fn row_to_document(row: &rusqlite::Row) -> Result<Document> {
    Ok(Document {
        workspace_id: row.get(0)?,
        path: row.get(1)?,
        local_revision: row.get(2)?,
        remote_revision: row.get(3)?,
        checksum: row.get(4)?,
        dirty: row.get(5)?,
        deleted: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

// --- Document Sections ---

pub fn upsert_sections(
    conn: &Connection,
    workspace_id: &str,
    document_path: &str,
    sections: &[Section],
    updated_at: &str,
) -> Result<()> {
    // Clear existing sections for this document first.
    conn.execute(
        "DELETE FROM document_sections WHERE workspace_id = ?1 AND document_path = ?2",
        params![workspace_id, document_path],
    )?;

    let mut stmt = conn.prepare(
        "INSERT INTO document_sections (
            workspace_id, document_path, section_path, heading_text,
            level, content_hash, ordinal, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    for section in sections {
        stmt.execute(params![
            workspace_id,
            document_path,
            section.path,
            section.heading_text,
            section.level,
            section.content_hash,
            section.ordinal,
            updated_at,
        ])?;
    }
    Ok(())
}

pub fn get_sections(
    conn: &Connection,
    workspace_id: &str,
    document_path: &str,
) -> Result<Vec<Section>> {
    let mut stmt = conn.prepare(
        "SELECT section_path, heading_text, level, content_hash, ordinal
         FROM document_sections
         WHERE workspace_id = ?1 AND document_path = ?2
         ORDER BY rowid",
    )?;

    let rows = stmt.query_map(params![workspace_id, document_path], |row| {
        let content_hash: String = row.get(3)?;
        Ok(Section {
            path: row.get(0)?,
            heading_text: row.get(1)?,
            level: row.get::<_, u8>(2)?,
            content: String::new(), // Content not stored in sections table
            content_hash,
            ordinal: row.get(4)?,
        })
    })?;

    let mut sections = Vec::new();
    for s in rows {
        sections.push(s?);
    }
    Ok(sections)
}

// --- Document Versions ---

pub fn save_version(
    conn: &Connection,
    workspace_id: &str,
    document_path: &str,
    revision: &str,
    content: &str,
    source: &str,
) -> Result<String> {
    let id = format!("dv_{}", nanoid::nanoid!(12));
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO document_versions (
            id, workspace_id, document_path, revision, content, source, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, workspace_id, document_path, revision, content, source, now],
    )?;
    Ok(id)
}

pub fn get_version(
    conn: &Connection,
    workspace_id: &str,
    document_path: &str,
    revision: &str,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT content FROM document_versions
         WHERE workspace_id = ?1 AND document_path = ?2 AND revision = ?3
         ORDER BY created_at DESC LIMIT 1",
    )?;
    let mut rows = stmt.query(params![workspace_id, document_path, revision])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}
