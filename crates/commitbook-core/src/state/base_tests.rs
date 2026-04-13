use super::*;

#[test]
fn test_read_missing_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("local").join("base")).unwrap();
    let result = read(tmp.path(), "nonexistent.md").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_write_and_read_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let content = "# Hello\n\nWorld\n";
    write(tmp.path(), "notes.md", content).unwrap();

    let loaded = read(tmp.path(), "notes.md").unwrap();
    assert_eq!(loaded.as_deref(), Some(content));
}

#[test]
fn test_write_creates_nested_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "deep/nested/file.md", "content").unwrap();

    let loaded = read(tmp.path(), "deep/nested/file.md").unwrap();
    assert_eq!(loaded.as_deref(), Some("content"));
}

#[test]
fn test_delete_removes_file() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "to-delete.md", "content").unwrap();
    assert!(read(tmp.path(), "to-delete.md").unwrap().is_some());

    delete(tmp.path(), "to-delete.md").unwrap();
    assert!(read(tmp.path(), "to-delete.md").unwrap().is_none());
}

#[test]
fn test_delete_nonexistent_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("local").join("base")).unwrap();
    // Should not error.
    delete(tmp.path(), "nonexistent.md").unwrap();
}

#[test]
fn test_list_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let files = list(tmp.path()).unwrap();
    assert!(files.is_empty());
}

#[test]
fn test_list_with_files() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.md", "aaa").unwrap();
    write(tmp.path(), "sub/b.md", "bbb").unwrap();

    let mut files = list(tmp.path()).unwrap();
    files.sort();
    assert_eq!(files, vec!["a.md", "sub/b.md"]);
}

#[test]
fn test_write_overwrites_existing() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "file.md", "v1").unwrap();
    write(tmp.path(), "file.md", "v2").unwrap();

    let loaded = read(tmp.path(), "file.md").unwrap();
    assert_eq!(loaded.as_deref(), Some("v2"));
}
