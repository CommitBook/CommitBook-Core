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
    symlink(outside.path(), root.path().join("owner__repo")).unwrap();
    let client = CommitBookEngineClient::new(
        "unused".to_string(),
        root.path().to_string_lossy().into_owned(),
        None,
    )
    .unwrap();

    let error = client
        .delete_commitbook("owner/repo".to_string())
        .expect_err("symlinked clone must not be registered or deleted");
    assert!(matches!(error, CommitBookError::NotFound { .. }));
    assert!(outside.path().join(".CommitBook/config.toml").exists());
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
        .map(|cb| cb.id)
        .collect();
    assert_eq!(listed, ["owner/notes"]);
    let broken = client.list_broken_commitbooks().unwrap();
    assert_eq!(broken.len(), 1);
    assert!(broken[0].path.ends_with("owner__old"), "{:?}", broken[0]);
    assert!(
        broken[0].error.contains("commitbook init"),
        "{:?}",
        broken[0]
    );
}
