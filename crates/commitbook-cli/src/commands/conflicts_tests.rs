use super::*;

#[test]
fn test_no_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("clean.md"), "# Hello\n\nNo conflicts.\n").unwrap();

    let mut results = Vec::new();
    scan_for_conflicts(tmp.path(), tmp.path(), &mut results).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_detects_conflict_markers() {
    let tmp = tempfile::tempdir().unwrap();
    let content =
        "# Title\n\n<<<<<<< LOCAL\nmy version\n=======\ntheir version\n>>>>>>> REMOTE\n";
    std::fs::write(tmp.path().join("conflict.md"), content).unwrap();

    let mut results = Vec::new();
    scan_for_conflicts(tmp.path(), tmp.path(), &mut results).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "conflict.md");
    assert_eq!(results[0].1, 1);
}

#[test]
fn test_multiple_conflicts_in_one_file() {
    let tmp = tempfile::tempdir().unwrap();
    let content = "<<<<<<< LOCAL\na\n>>>>>>>\n\n<<<<<<< LOCAL\nb\n>>>>>>>\n";
    std::fs::write(tmp.path().join("multi.md"), content).unwrap();

    let mut results = Vec::new();
    scan_for_conflicts(tmp.path(), tmp.path(), &mut results).unwrap();
    assert_eq!(results[0].1, 2);
}

#[test]
fn test_ignores_non_markdown() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("code.rs"), "<<<<<<< LOCAL\nconflict\n").unwrap();

    let mut results = Vec::new();
    scan_for_conflicts(tmp.path(), tmp.path(), &mut results).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_skips_hidden_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let hidden = tmp.path().join(".hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("file.md"), "<<<<<<< LOCAL\n").unwrap();

    let mut results = Vec::new();
    scan_for_conflicts(tmp.path(), tmp.path(), &mut results).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_nested_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("nested.md"), "<<<<<<< LOCAL\ndata\n").unwrap();

    let mut results = Vec::new();
    scan_for_conflicts(tmp.path(), tmp.path(), &mut results).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "sub/nested.md");
}
