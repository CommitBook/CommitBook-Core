use super::*;

#[test]
fn creates_parent_directories_before_locking() {
    let tmp = tempfile::tempdir().unwrap();
    let lock = RepoLock::acquire(tmp.path()).unwrap();
    assert!(LocalConfig::lock_path(tmp.path()).exists());
    drop(lock);
}

#[test]
fn rejects_concurrent_mutation_and_releases_on_drop() {
    let tmp = tempfile::tempdir().unwrap();
    let first = RepoLock::acquire(tmp.path()).unwrap();
    let error = RepoLock::acquire(tmp.path()).unwrap_err();
    assert!(error.downcast_ref::<RepoLockContended>().is_some());
    drop(first);
    RepoLock::acquire(tmp.path()).unwrap();
}

#[test]
fn releases_during_panic_unwinding() {
    let tmp = tempfile::tempdir().unwrap();
    let result = std::panic::catch_unwind(|| {
        let _lock = RepoLock::acquire(tmp.path()).unwrap();
        panic!("exercise lock guard unwind");
    });
    assert!(result.is_err());

    RepoLock::acquire(tmp.path()).unwrap();
}

#[test]
fn concurrent_first_acquires_reach_the_repository_lock() {
    use std::sync::{Arc, Barrier};

    let tmp = tempfile::tempdir().unwrap();
    let root = Arc::new(tmp.path().to_path_buf());
    let start = Arc::new(Barrier::new(3));
    let finish = Arc::new(Barrier::new(3));
    let mut threads = Vec::new();

    for _ in 0..2 {
        let root = Arc::clone(&root);
        let start = Arc::clone(&start);
        let finish = Arc::clone(&finish);
        threads.push(std::thread::spawn(move || {
            start.wait();
            let result = RepoLock::acquire(&root);
            let classification = match &result {
                Ok(_) => "acquired",
                Err(error) if error.downcast_ref::<RepoLockContended>().is_some() => "contended",
                Err(_) => "layout-error",
            };
            finish.wait();
            classification
        }));
    }

    start.wait();
    finish.wait();
    let mut outcomes = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    outcomes.sort_unstable();
    assert_eq!(outcomes, ["acquired", "contended"]);
}

#[test]
fn coordinates_with_legacy_lock() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".CommitBook")).unwrap();
    let legacy_path = tmp.path().join(".CommitBook/.lock");
    let legacy_auth = tmp.path().join(".CommitBook/auth.toml");
    std::fs::write(&legacy_auth, "token = \"legacy\"\n").unwrap();
    let legacy = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&legacy_path)
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&legacy).unwrap();

    let error = RepoLock::acquire(tmp.path()).unwrap_err();
    assert!(error.downcast_ref::<RepoLockContended>().is_some());
    assert!(legacy_auth.exists());
    assert!(!tmp.path().join(".CommitBook/local/auth.toml").exists());
    fs2::FileExt::unlock(&legacy).unwrap();
    drop(legacy);

    let migrated = RepoLock::acquire(tmp.path()).unwrap();
    assert!(legacy_path.exists());
    assert!(LocalConfig::lock_path(tmp.path()).exists());
    assert!(!legacy_auth.exists());
    assert!(tmp.path().join(".CommitBook/local/auth.toml").exists());
    drop(migrated);
    assert!(legacy_path.exists());
}

#[test]
fn rejects_a_lock_for_a_different_repository() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let lock = RepoLock::acquire(first.path()).unwrap();

    let error = lock.ensure_matches(second.path()).unwrap_err().to_string();
    assert!(error.contains("cannot guard operation"));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_legacy_lock_without_touching_target() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".CommitBook")).unwrap();
    symlink(outside.path(), tmp.path().join(".CommitBook/.lock")).unwrap();

    let error = RepoLock::acquire(tmp.path()).unwrap_err().to_string();
    assert!(error.contains("not a regular file"));
    assert!(std::fs::read(outside.path()).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_primary_lock_without_touching_target() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".CommitBook/local/logs")).unwrap();
    symlink(outside.path(), tmp.path().join(".CommitBook/local/.lock")).unwrap();

    let error = RepoLock::acquire(tmp.path()).unwrap_err().to_string();
    assert!(error.contains("not a regular file"));
    assert!(std::fs::read(outside.path()).unwrap().is_empty());
}

#[test]
fn fresh_repository_creates_only_the_primary_lock_file() {
    let tmp = tempfile::tempdir().unwrap();
    let legacy_path = LocalConfig::commitbook_dir(tmp.path()).join(".lock");

    let lock = RepoLock::acquire(tmp.path()).unwrap();
    assert!(LocalConfig::lock_path(tmp.path()).exists());
    assert!(!legacy_path.exists());
    drop(lock);

    assert!(LocalConfig::lock_path(tmp.path()).exists());
    assert!(!legacy_path.exists());
}
