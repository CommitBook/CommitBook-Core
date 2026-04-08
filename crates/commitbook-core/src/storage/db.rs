use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

use super::migrations;

/// Opens (or creates) a SQLite database at the given path and runs pending migrations.
pub fn open_database(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("Failed to open database at {}", path.display()))?;
    configure_connection(&conn)?;
    migrations::migrate_to_latest(&conn)?;
    Ok(conn)
}

/// Opens an in-memory SQLite database for testing.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory().context("Failed to open in-memory database")?;
    configure_connection(&conn)?;
    migrations::migrate_to_latest(&conn)?;
    Ok(conn)
}

fn configure_connection(conn: &Connection) -> Result<()> {
    // Enable WAL mode for concurrent readers.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // Enable foreign key enforcement.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

#[cfg(test)]
#[path = "db_tests.rs"]
mod tests;
