use super::*;

#[test]
fn test_default_state() {
    let state = SyncState::default();
    assert!(state.last_sync_at.is_none());
    assert!(state.last_error.is_none());
}

#[test]
fn test_load_missing_returns_default() {
    let tmp = tempfile::tempdir().unwrap();
    let state = SyncState::load(tmp.path()).unwrap();
    assert!(state.last_sync_at.is_none());
    assert!(state.last_error.is_none());
}

#[test]
fn test_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let state = SyncState {
        last_sync_at: Some("2026-04-09T10:00:00Z".to_string()),
        last_error: None,
        pending_init_push: None,
        ..Default::default()
    };
    state.save(tmp.path()).unwrap();

    let loaded = SyncState::load(tmp.path()).unwrap();
    assert_eq!(loaded.last_sync_at.as_deref(), Some("2026-04-09T10:00:00Z"));
    assert!(loaded.last_error.is_none());
}

#[test]
fn test_save_creates_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("local").join("state.toml");
    assert!(!path.exists());

    SyncState::default().save(tmp.path()).unwrap();
    assert!(path.exists());
}

#[test]
fn test_save_overwrites_existing() {
    let tmp = tempfile::tempdir().unwrap();

    let state1 = SyncState {
        last_sync_at: Some("2026-01-01T00:00:00Z".to_string()),
        last_error: Some("first error".to_string()),
        pending_init_push: None,
        ..Default::default()
    };
    state1.save(tmp.path()).unwrap();

    let state2 = SyncState {
        last_sync_at: Some("2026-04-09T12:00:00Z".to_string()),
        last_error: None,
        pending_init_push: None,
        ..Default::default()
    };
    state2.save(tmp.path()).unwrap();

    let loaded = SyncState::load(tmp.path()).unwrap();
    assert_eq!(loaded.last_sync_at.as_deref(), Some("2026-04-09T12:00:00Z"));
    assert!(loaded.last_error.is_none());
}

#[test]
fn test_load_legacy_state_with_remote_head() {
    // Old state.toml files written before the SyncState shrink contained a
    // `remote_head` field. They must still load (silently ignored).
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("local")).unwrap();
    let legacy = r#"
remote_head = "abc123"
last_sync_at = "2026-04-09T10:00:00Z"
"#;
    std::fs::write(tmp.path().join("local").join("state.toml"), legacy).unwrap();

    let loaded = SyncState::load(tmp.path()).unwrap();
    assert_eq!(loaded.last_sync_at.as_deref(), Some("2026-04-09T10:00:00Z"));
}
