use super::*;

#[test]
fn safe_rel_join_accepts_normal_relative_path() {
    let base = Path::new("/tmp/clone");
    let joined = safe_rel_join(base, "notes/a.md").unwrap();
    assert_eq!(joined, Path::new("/tmp/clone/notes/a.md"));
}

#[test]
fn safe_rel_join_rejects_parent_traversal() {
    let base = Path::new("/tmp/clone");
    assert!(safe_rel_join(base, "../x").is_err());
    assert!(safe_rel_join(base, "notes/../../etc/passwd").is_err());
}

#[test]
fn safe_rel_join_rejects_absolute_path() {
    let base = Path::new("/tmp/clone");
    assert!(safe_rel_join(base, "/etc/passwd").is_err());
}

#[test]
fn safe_rel_join_rejects_dot_commitbook_segment() {
    let base = Path::new("/tmp/clone");
    assert!(safe_rel_join(base, ".CommitBook/local/auth.toml").is_err());
    // Case variants must also be rejected: on case-insensitive filesystems
    // they resolve to the real .CommitBook directory.
    assert!(safe_rel_join(base, ".commitbook/local/auth.toml").is_err());
    assert!(safe_rel_join(base, ".COMMITBOOK/config.toml").is_err());
}
