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
fn rejects_a_lock_for_a_different_repository() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let lock = RepoLock::acquire(first.path()).unwrap();

    let error = lock.ensure_matches(second.path()).unwrap_err().to_string();
    assert!(error.contains("cannot guard operation"));
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

#[test]
fn process_exit_releases_the_lock() {
    const CHILD_ROOT: &str = "COMMITBOOK_LOCK_TEST_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let _lock = RepoLock::acquire(&root).unwrap();
        std::fs::write(root.join("ready"), "ready").unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::lock::tests::process_exit_releases_the_lock",
        ])
        .env(CHILD_ROOT, tmp.path())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !tmp.path().join("ready").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let ready = tmp.path().join("ready").exists();
    let contended = ready && RepoLock::acquire(tmp.path()).is_err();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(ready && contended);
    assert!(LocalConfig::lock_path(tmp.path()).exists());
    RepoLock::acquire(tmp.path()).unwrap();
}
