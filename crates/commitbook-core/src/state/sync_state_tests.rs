use super::*;

#[test]
fn test_default_state() {
    let state = SyncState::default();
    assert!(state.remote_head.is_none());
    assert!(state.last_sync_at.is_none());
}

#[test]
fn test_load_missing_returns_default() {
    let tmp = tempfile::tempdir().unwrap();
    let state = SyncState::load(tmp.path()).unwrap();
    assert!(state.remote_head.is_none());
    assert!(state.last_sync_at.is_none());
}

#[test]
fn test_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let state = SyncState {
        remote_head: Some("abc123def456".to_string()),
        last_sync_at: Some("2026-04-09T10:00:00Z".to_string()),
    };
    state.save(tmp.path()).unwrap();

    let loaded = SyncState::load(tmp.path()).unwrap();
    assert_eq!(loaded.remote_head.as_deref(), Some("abc123def456"));
    assert_eq!(
        loaded.last_sync_at.as_deref(),
        Some("2026-04-09T10:00:00Z")
    );
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
        remote_head: Some("first".to_string()),
        last_sync_at: None,
    };
    state1.save(tmp.path()).unwrap();

    let state2 = SyncState {
        remote_head: Some("second".to_string()),
        last_sync_at: Some("2026-04-09T12:00:00Z".to_string()),
    };
    state2.save(tmp.path()).unwrap();

    let loaded = SyncState::load(tmp.path()).unwrap();
    assert_eq!(loaded.remote_head.as_deref(), Some("second"));
}
