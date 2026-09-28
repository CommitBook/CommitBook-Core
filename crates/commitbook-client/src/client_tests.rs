use super::*;

/// Guard for the shared-runtime wiring behind the four `[Async]` FFI methods.
///
/// UniFFI's UDL scaffolding polls `[Async]` methods on the foreign caller's
/// thread with no ambient tokio runtime, so the `reqwest` / `tokio::spawn` /
/// `spawn_blocking` calls inside these methods used to panic. Each method now
/// runs its body on the process-global runtime in `crate::runtime`.
///
/// `pollster::block_on` drives the future from a plain thread with no tokio
/// runtime installed, which is exactly that condition. `sync_commitbook` is
/// the method used here because it fails fast at the registry lookup in
/// `sync_ops::sync_one_commitbook` (filesystem only, no network) while still
/// exercising the full `runtime().spawn(..)` + `spawn_blocking` path.
#[test]
fn async_ffi_methods_run_without_ambient_tokio_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let client = CommitBookEngineClient::new(
        "unused-db-path".to_string(),
        tmp.path().to_string_lossy().into_owned(),
        None,
    )
    .unwrap();

    // A bogus id resolves to NotFound before any network call is attempted.
    // Pre-fix this line panicked instead of returning.
    let err = pollster::block_on(client.sync_commitbook(
        "does-not-exist".to_string(),
        SyncMode::Manual,
        "token".to_string(),
    ))
    .expect_err("a bogus commitbook id should error, not succeed");

    assert!(
        matches!(err, CommitBookError::NotFound { .. }),
        "expected NotFound, got: {err}"
    );

    // The shared runtime is a process-global OnceLock, so a second call must
    // reuse it rather than panic or deadlock.
    let second = pollster::block_on(client.sync_commitbook(
        "also-missing".to_string(),
        SyncMode::Manual,
        "token".to_string(),
    ));
    assert!(
        matches!(second, Err(CommitBookError::NotFound { .. })),
        "second call should also return NotFound, got: {second:?}"
    );
}

#[cfg(unix)]
#[test]
fn delete_never_follows_workspace_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    git2::Repository::init(outside.path())
        .unwrap()
        .remote("origin", "https://github.com/owner/repo.git")
        .unwrap();
    commitbook_engine::commitbooks::init_dot_commitbook(
        outside.path(),
        "Outside",
        "main",
        "origin",
        None,
        commitbook_engine::config::Auth::Pat,
    )
    .unwrap();
    crate::test_support::set_identity(outside.path());
    symlink(outside.path(), root.path().join("owner__repo")).unwrap();
    let client = CommitBookEngineClient::new(
        "unused".to_string(),
        root.path().to_string_lossy().into_owned(),
        None,
    )
    .unwrap();

    let error = client
        .delete_commitbook("a1b2c3d4".to_string(), false)
        .expect_err("symlinked clone must not be registered or deleted");
    assert!(matches!(error, CommitBookError::NotFound { .. }));
    assert!(outside.path().join(".CommitBook/config.toml").exists());
}

fn deletion_fixture() -> (tempfile::TempDir, CommitBookEngineClient, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let clone = root.path().join("owner__repo");
    let repo = git2::Repository::init(&clone).unwrap();
    repo.set_head("refs/heads/main").unwrap();
    repo.remote("origin", "https://github.com/owner/repo.git")
        .unwrap();
    commitbook_engine::config::LocalConfig::new("Notes", "main", "origin")
        .save(&clone)
        .unwrap();
    crate::test_support::set_identity(&clone);
    commitbook_engine::config::LocalConfig::ensure_gitignore(&clone).unwrap();
    std::fs::write(clone.join("note.md"), "published\n").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_path(std::path::Path::new(".CommitBook/config.toml"))
        .unwrap();
    index
        .add_path(std::path::Path::new(".CommitBook/.gitignore"))
        .unwrap();
    index.add_path(std::path::Path::new("note.md")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Test", "test@example.com").unwrap();
    let published = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "published",
            &tree,
            &[],
        )
        .unwrap();
    repo.reference("refs/remotes/origin/main", published, true, "test")
        .unwrap();
    assert!(
        !commitbook_engine::git::GitRepo::open(&clone)
            .unwrap()
            .has_dirty_changes()
            .unwrap(),
        "published fixture must start clean"
    );
    let client = CommitBookEngineClient::new(
        "unused".to_string(),
        root.path().to_string_lossy().into_owned(),
        None,
    )
    .unwrap();
    (root, client, clone)
}

fn assert_delete_refused(client: &CommitBookEngineClient, clone: &std::path::Path, reason: &str) {
    let error = client
        .delete_commitbook("a1b2c3d4".to_string(), false)
        .expect_err("unsaved work must prevent deletion");
    assert!(
        matches!(error, CommitBookError::InvalidInput { .. }),
        "{error}"
    );
    assert!(error.to_string().contains(reason), "{error}");
    assert!(clone.exists(), "refused deletion must leave clone intact");
}

#[test]
fn delete_clean_clone() {
    let (_root, client, clone) = deletion_fixture();
    client
        .delete_commitbook("a1b2c3d4".to_string(), false)
        .unwrap();
    assert!(!clone.exists());
}

#[test]
fn delete_refuses_uncommitted_changes() {
    let (_root, client, clone) = deletion_fixture();
    std::fs::write(clone.join("draft.md"), "uncommitted\n").unwrap();
    assert_delete_refused(&client, &clone, "never committed");
}

#[test]
fn delete_refuses_git_operation_in_progress() {
    let (_root, client, clone) = deletion_fixture();
    let repo = git2::Repository::open(&clone).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    std::fs::write(repo.path().join("MERGE_HEAD"), format!("{head}\n")).unwrap();
    assert_delete_refused(&client, &clone, "git operation in progress");
}

#[test]
fn delete_refuses_unpushed_commits() {
    let (_root, client, clone) = deletion_fixture();
    let repo = git2::Repository::open(&clone).unwrap();
    std::fs::write(clone.join("note.md"), "local edit\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(std::path::Path::new("note.md")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let parent = repo.head().unwrap().peel_to_commit().unwrap();
    let signature = git2::Signature::now("Test", "test@example.com").unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "local",
        &tree,
        &[&parent],
    )
    .unwrap();
    assert_delete_refused(&client, &clone, "1 commit(s) not pushed");
}

#[test]
fn delete_refuses_missing_tracking_ref() {
    let (_root, client, clone) = deletion_fixture();
    git2::Repository::open(&clone)
        .unwrap()
        .find_reference("refs/remotes/origin/main")
        .unwrap()
        .delete()
        .unwrap();
    assert_delete_refused(&client, &clone, "no local record");
}

#[test]
fn force_delete_discards_uncommitted_work() {
    let (_root, client, clone) = deletion_fixture();
    std::fs::write(clone.join("draft.md"), "uncommitted\n").unwrap();
    client
        .delete_commitbook("a1b2c3d4".to_string(), true)
        .unwrap();
    assert!(!clone.exists());
}

#[cfg(unix)]
#[test]
fn constructor_rejects_symlink_workspace_root() {
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let linked = parent.path().join("linked-root");
    symlink(outside.path(), &linked).unwrap();
    assert!(CommitBookEngineClient::new(
        "unused".to_string(),
        linked.to_string_lossy().into_owned(),
        None,
    )
    .is_err());
}

#[test]
fn a_broken_clone_is_listed_separately_and_does_not_hide_the_others() {
    let root = tempfile::tempdir().unwrap();
    let good = root.path().join("owner__notes");
    std::fs::create_dir(&good).unwrap();
    git2::Repository::init(&good)
        .unwrap()
        .remote("origin", "https://github.com/owner/notes.git")
        .unwrap();
    commitbook_engine::config::LocalConfig::new("Notes", "main", "origin")
        .save(&good)
        .unwrap();
    crate::test_support::set_identity(&good);
    let old = root.path().join("owner__old");
    std::fs::create_dir_all(old.join(".CommitBook")).unwrap();
    git2::Repository::init(&old).unwrap();
    std::fs::write(
        old.join(".CommitBook/config.toml"),
        "config_version = \"1\"\n",
    )
    .unwrap();

    let client = CommitBookEngineClient::new(
        "unused-db-path".to_string(),
        root.path().to_string_lossy().into_owned(),
        None,
    )
    .unwrap();
    let listed: Vec<String> = client
        .list_commitbooks()
        .unwrap()
        .into_iter()
        .map(|cb| cb.commitbook_id)
        .collect();
    assert_eq!(listed, ["a1b2c3d4"]);
    let broken = client.list_broken_commitbooks().unwrap();
    assert_eq!(broken.len(), 1);
    assert!(broken[0].path.ends_with("owner__old"), "{:?}", broken[0]);
    assert!(
        broken[0].error.contains("commitbook init"),
        "{:?}",
        broken[0]
    );
}

#[test]
fn registration_is_local_read_listing_is_pure_and_paths_are_constrained() {
    let (root, client, clone) = deletion_fixture();
    let id_path = commitbook_engine::commitbooks::identity::path(&clone);
    std::fs::remove_file(&id_path).unwrap();
    assert!(client.list_commitbooks().unwrap().is_empty());
    assert_eq!(client.list_broken_commitbooks().unwrap().len(), 1);
    assert!(!id_path.exists());
    let before = git2::Repository::open(&clone)
        .unwrap()
        .head()
        .unwrap()
        .target();
    let summary = client
        .register_local_commitbook("owner__repo".into())
        .unwrap();
    assert_eq!(summary.commitbook_id.len(), 8);
    assert_eq!(
        before,
        git2::Repository::open(&clone)
            .unwrap()
            .head()
            .unwrap()
            .target()
    );
    assert_eq!(
        summary.commitbook_id,
        client
            .register_local_commitbook("owner__repo".into())
            .unwrap()
            .commitbook_id
    );
    assert!(client.get_commitbook("owner/repo".into()).is_err());
    for path in ["../outside", "/tmp", "owner__repo/../owner__repo", ".", ""] {
        assert!(client.register_local_commitbook(path.into()).is_err());
    }
    let moved = root.path().join("renamed");
    std::fs::rename(&clone, &moved).unwrap();
    assert_eq!(
        client
            .get_commitbook(summary.commitbook_id.clone())
            .unwrap()
            .commitbook_id,
        summary.commitbook_id
    );
}
