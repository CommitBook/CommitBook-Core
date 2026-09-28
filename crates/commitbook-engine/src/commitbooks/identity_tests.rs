use super::*;

#[test]
fn identity_is_persistent_local_and_survives_move() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("before");
    std::fs::create_dir(&root).unwrap();
    git2::Repository::init(&root).unwrap();
    LocalConfig::new("Notes", "main", "origin")
        .save(&root)
        .unwrap();
    let id = ensure(&root).unwrap();
    validate(&id).unwrap();
    assert_eq!(id, ensure(&root).unwrap());
    assert_eq!(
        std::fs::read_to_string(path(&root)).unwrap(),
        format!("CommitBook-Id = \"{id}\"\n")
    );
    let moved = parent.path().join("after");
    std::fs::rename(&root, &moved).unwrap();
    LocalConfig::new("Renamed", "main", "different")
        .save(&moved)
        .unwrap();
    assert_eq!(id, ensure(&moved).unwrap());
    let repo = crate::git::GitRepo::open(&moved).unwrap();
    repo.stage_all().unwrap();
    assert!(git2::Repository::open(&moved)
        .unwrap()
        .index()
        .unwrap()
        .get_path(Path::new(".CommitBook/local/CommitBook-ID.toml"), 0)
        .is_none());
}

#[test]
fn fresh_clones_and_recreated_identity_use_randomness() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let first = ensure(a.path()).unwrap();
    assert_ne!(first, ensure(b.path()).unwrap());
    std::fs::remove_file(path(a.path())).unwrap();
    assert_ne!(first, ensure(a.path()).unwrap());
}

#[test]
fn missing_reads_do_not_write_and_malformed_files_are_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(load(tmp.path()).is_err());
    assert!(!LocalConfig::commitbook_dir(tmp.path()).exists());
    ensure(tmp.path()).unwrap();
    for bad in [
        "id = \"a7c39e2b\"",
        "CommitBook-Id = \"ABCDEF01\"",
        "CommitBook-Id = \"123\"",
        "bad toml",
    ] {
        std::fs::write(path(tmp.path()), bad).unwrap();
        assert!(ensure(tmp.path()).is_err());
        assert_eq!(std::fs::read_to_string(path(tmp.path())).unwrap(), bad);
    }
}

#[cfg(unix)]
#[test]
fn symlinked_file_or_parent_is_rejected() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    ensure(tmp.path()).unwrap();
    std::fs::write(
        outside.path().join(FILE_NAME),
        "CommitBook-Id = \"a7c39e2b\"\n",
    )
    .unwrap();
    std::fs::remove_file(path(tmp.path())).unwrap();
    symlink(outside.path().join(FILE_NAME), path(tmp.path())).unwrap();
    assert!(ensure(tmp.path()).is_err());
    std::fs::remove_dir_all(LocalConfig::local_dir(tmp.path())).unwrap();
    symlink(outside.path(), LocalConfig::local_dir(tmp.path())).unwrap();
    assert!(load(tmp.path()).is_err());
    assert!(ensure(tmp.path()).is_err());
}

#[test]
fn initialization_requires_the_matching_exclusive_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let lock = RepoLock::acquire(tmp.path()).unwrap();
    assert!(ensure(tmp.path()).is_err());
    assert!(ensure_locked(other.path(), &lock).is_err());
    let first = ensure_locked(tmp.path(), &lock).unwrap();
    drop(lock);
    assert_eq!(first, ensure(tmp.path()).unwrap());
}
