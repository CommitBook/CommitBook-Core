use super::*;

#[test]
fn validates_github_components_and_branch_names() {
    assert!(validate_init_input("openai", "commit-book_2.0", "feature/mobile").is_ok());
    for value in [
        "",
        ".",
        "..",
        "...",
        "../owner",
        "owner/repo",
        "owner\nname",
    ] {
        assert!(
            validate_repo_component("owner", value).is_err(),
            "{value:?}"
        );
    }
    for branch in [
        "",
        "../main",
        "bad..name",
        "main.lock",
        "bad name",
        "bad\0name",
    ] {
        assert!(validate_branch(branch).is_err(), "{branch:?}");
    }
}

#[test]
fn markdown_paths_reject_protected_and_non_markdown_targets() {
    assert!(validate_markdown_relative("notes/day.md").is_ok());
    assert!(validate_markdown_relative("notes/day.MARKDOWN").is_ok());
    for value in [
        "../secret.md",
        "/tmp/file.md",
        ".git/config.md",
        ".GIT/config.md",
        ".CommitBook/local/auth.md",
        "notes/./day.md",
        "notes//day.md",
        "notes.txt",
    ] {
        assert!(validate_markdown_relative(value).is_err(), "{value:?}");
    }
}

#[cfg(unix)]
#[test]
fn safe_document_path_rejects_symlink_components() {
    use std::os::unix::fs::symlink;

    let clone = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), clone.path().join("notes")).unwrap();
    assert!(safe_document_path(clone.path(), "notes/secret.md", true).is_err());
    assert!(!outside.path().join("secret.md").exists());
}

#[cfg(unix)]
#[test]
fn managed_clone_must_not_be_a_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join("owner__repo")).unwrap();
    assert!(validate_managed_clone(root.path(), &root.path().join("owner__repo")).is_err());
}
