use super::*;

#[test]
fn test_open_in_memory() {
    let conn = open_in_memory().unwrap();
    // Verify WAL mode is set.
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert!(mode == "wal" || mode == "memory"); // in-memory may report "memory"
}

#[test]
fn test_foreign_keys_enabled() {
    let conn = open_in_memory().unwrap();
    let fk: i32 = conn
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .unwrap();
    assert_eq!(fk, 1);
}

#[test]
fn test_schema_version_is_1() {
    let conn = open_in_memory().unwrap();
    let version: u32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn test_tables_exist() {
    let conn = open_in_memory().unwrap();
    let tables = vec![
        "workspaces",
        "documents",
        "document_sections",
        "document_versions",
        "sync_checkpoints",
        "sync_jobs",
        "sync_events",
        "conflicts",
        "workspace_settings",
    ];
    for table in tables {
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "Table '{}' should exist", table);
    }
}

#[test]
fn test_migration_idempotent() {
    let conn = open_in_memory().unwrap();
    // Running migrate_to_latest again should not fail.
    migrations::migrate_to_latest(&conn).unwrap();
    let version: u32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn test_open_file_database() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");
    let conn = open_database(&db_path).unwrap();
    let version: u32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 1);
    assert!(db_path.exists());
}
