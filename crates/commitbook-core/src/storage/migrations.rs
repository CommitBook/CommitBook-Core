use anyhow::{Context, Result};
use rusqlite::Connection;

/// Current schema version.
const LATEST_VERSION: u32 = 1;

/// Run all pending forward-only migrations.
pub fn migrate_to_latest(conn: &Connection) -> Result<()> {
    ensure_version_table(conn)?;
    let current = current_version(conn)?;

    if current < 1 {
        migrate_v1(conn).context("Migration V1 failed")?;
    }

    Ok(())
}

fn ensure_version_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;
    Ok(())
}

fn current_version(conn: &Connection) -> Result<u32> {
    let version: Option<u32> = conn.query_row(
        "SELECT MAX(version) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    Ok(version.unwrap_or(0))
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE workspaces (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            mode TEXT NOT NULL,
            provider TEXT NOT NULL,
            remote_url TEXT,
            owner TEXT,
            repo_name TEXT,
            branch TEXT NOT NULL DEFAULT 'main',
            local_mode TEXT NOT NULL DEFAULT 'sandbox',
            local_root TEXT NOT NULL,
            merge_mode TEXT NOT NULL DEFAULT 'section_aware',
            sync_interval_seconds INTEGER NOT NULL DEFAULT 300,
            auto_sync INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE documents (
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            path TEXT NOT NULL,
            local_revision TEXT,
            remote_revision TEXT,
            checksum TEXT NOT NULL,
            dirty INTEGER NOT NULL DEFAULT 0,
            deleted INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (workspace_id, path)
        );

        CREATE TABLE document_sections (
            workspace_id TEXT NOT NULL,
            document_path TEXT NOT NULL,
            section_path TEXT NOT NULL,
            heading_text TEXT NOT NULL,
            level INTEGER NOT NULL,
            content_hash TEXT NOT NULL,
            ordinal INTEGER,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (workspace_id, document_path, section_path),
            FOREIGN KEY (workspace_id, document_path)
                REFERENCES documents(workspace_id, path) ON DELETE CASCADE
        );

        CREATE TABLE document_versions (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            document_path TEXT NOT NULL,
            revision TEXT NOT NULL,
            content TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY (workspace_id, document_path)
                REFERENCES documents(workspace_id, path) ON DELETE CASCADE
        );
        CREATE INDEX idx_doc_versions_lookup
            ON document_versions(workspace_id, document_path, revision);

        CREATE TABLE sync_checkpoints (
            workspace_id TEXT PRIMARY KEY
                REFERENCES workspaces(id) ON DELETE CASCADE,
            remote_head TEXT NOT NULL,
            last_completed_at TEXT NOT NULL
        );

        CREATE TABLE sync_jobs (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            job_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            retry_count INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 3,
            error_message TEXT,
            created_at TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT
        );
        CREATE INDEX idx_sync_jobs_workspace
            ON sync_jobs(workspace_id, status);

        CREATE TABLE sync_events (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            job_id TEXT,
            category TEXT NOT NULL,
            action TEXT NOT NULL,
            result TEXT NOT NULL,
            duration_ms INTEGER,
            error_code TEXT,
            payload_json TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX idx_sync_events_workspace
            ON sync_events(workspace_id, created_at);

        CREATE TABLE conflicts (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            path TEXT NOT NULL,
            section_path TEXT,
            conflict_type TEXT NOT NULL,
            base_content TEXT,
            local_content TEXT NOT NULL,
            remote_content TEXT NOT NULL,
            merged_preview TEXT,
            status TEXT NOT NULL DEFAULT 'open',
            resolution_type TEXT,
            opened_at TEXT NOT NULL,
            resolved_at TEXT
        );
        CREATE INDEX idx_conflicts_workspace
            ON conflicts(workspace_id, status);

        CREATE TABLE workspace_settings (
            workspace_id TEXT PRIMARY KEY
                REFERENCES workspaces(id) ON DELETE CASCADE,
            auth_ref TEXT,
            settings_json TEXT NOT NULL DEFAULT '{}'
        );

        INSERT INTO schema_version (version, applied_at)
            VALUES (1, datetime('now'));
        ",
    )?;
    Ok(())
}

/// Returns the latest schema version constant.
pub fn latest_version() -> u32 {
    LATEST_VERSION
}
