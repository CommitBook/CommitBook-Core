use super::*;

#[test]
fn test_engine_in_memory() {
    let engine = CommitBookEngine::in_memory().unwrap();
    let workspaces = engine.list_workspaces().unwrap();
    assert!(workspaces.is_empty());
}

#[test]
fn test_create_and_list_workspace() {
    let engine = CommitBookEngine::in_memory().unwrap();

    let ws = engine
        .create_workspace(
            "Test Notes",
            "existing_local_repo",
            "generic_git",
            None,
            None,
            None,
            "main",
            "/tmp/test-notes",
        )
        .unwrap();

    assert!(ws.id.starts_with("wk_"));
    assert_eq!(ws.name, "Test Notes");
    assert_eq!(ws.mode, "existing_local_repo");
    assert_eq!(ws.doc_count, 0);
    assert_eq!(ws.conflict_count, 0);

    let list = engine.list_workspaces().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, ws.id);
}

#[test]
fn test_get_workspace() {
    let engine = CommitBookEngine::in_memory().unwrap();

    let ws = engine
        .create_workspace(
            "Test",
            "existing_local_repo",
            "github",
            None,
            None,
            None,
            "main",
            "/tmp/test",
        )
        .unwrap();

    let loaded = engine.get_workspace(&ws.id).unwrap();
    assert_eq!(loaded.name, "Test");
    assert_eq!(loaded.provider, "github");
}

#[test]
fn test_get_workspace_not_found() {
    let engine = CommitBookEngine::in_memory().unwrap();
    assert!(engine.get_workspace("nonexistent").is_err());
}

#[test]
fn test_delete_workspace() {
    let engine = CommitBookEngine::in_memory().unwrap();

    let ws = engine
        .create_workspace(
            "Delete Me",
            "existing_local_repo",
            "generic_git",
            None,
            None,
            None,
            "main",
            "/tmp/del",
        )
        .unwrap();

    engine.delete_workspace(&ws.id).unwrap();
    let list = engine.list_workspaces().unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_save_and_read_document() {
    let engine = CommitBookEngine::in_memory().unwrap();

    let ws = engine
        .create_workspace(
            "Notes",
            "existing_local_repo",
            "generic_git",
            None,
            None,
            None,
            "main",
            "/tmp/notes",
        )
        .unwrap();

    engine
        .save_document(&ws.id, "notes.md", "# Notes\n\nHello world.\n")
        .unwrap();

    let docs = engine.list_documents(&ws.id).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].path, "notes.md");
    assert!(docs[0].dirty);

    let _content = engine.read_document(&ws.id, "notes.md").unwrap();
}

#[test]
fn test_list_documents_empty() {
    let engine = CommitBookEngine::in_memory().unwrap();

    let ws = engine
        .create_workspace(
            "Empty",
            "existing_local_repo",
            "generic_git",
            None,
            None,
            None,
            "main",
            "/tmp/empty",
        )
        .unwrap();

    let docs = engine.list_documents(&ws.id).unwrap();
    assert!(docs.is_empty());
}

#[test]
fn test_list_conflicts_empty() {
    let engine = CommitBookEngine::in_memory().unwrap();

    let ws = engine
        .create_workspace(
            "NoConflicts",
            "existing_local_repo",
            "generic_git",
            None,
            None,
            None,
            "main",
            "/tmp/nc",
        )
        .unwrap();

    let conflicts = engine.list_conflicts(&ws.id).unwrap();
    assert!(conflicts.is_empty());
}

#[test]
fn test_create_workspace_invalid_mode() {
    let engine = CommitBookEngine::in_memory().unwrap();
    assert!(engine
        .create_workspace(
            "Bad",
            "invalid_mode",
            "generic_git",
            None,
            None,
            None,
            "main",
            "/tmp/bad",
        )
        .is_err());
}

#[test]
fn test_multiple_workspaces() {
    let engine = CommitBookEngine::in_memory().unwrap();

    engine
        .create_workspace("WS1", "existing_local_repo", "github", None, None, None, "main", "/tmp/ws1")
        .unwrap();
    engine
        .create_workspace("WS2", "ssh", "gitlab", Some("git@gitlab.com:user/repo.git"), None, None, "main", "/tmp/ws2")
        .unwrap();
    engine
        .create_workspace("WS3", "pat", "github", None, Some("user"), Some("repo"), "develop", "/tmp/ws3")
        .unwrap();

    let list = engine.list_workspaces().unwrap();
    assert_eq!(list.len(), 3);
}
