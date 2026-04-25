use super::*;
use crate::git::test_support::setup_repo_with_base;
use git2::Repository;

fn head_message(repo_path: &std::path::Path) -> String {
    let repo = Repository::open(repo_path).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    head.message().unwrap_or_default().to_string()
}

#[test]
fn test_commit_files_returns_only_changed_paths() {
    // 20 inputs, 19 identical to HEAD, 1 real change.
    // Pre-populate the base with 20 files at known content, then feed the
    // same content back in for 19 of them and a real change for the 20th.
    let files: Vec<(String, String)> = (0..20)
        .map(|i| (format!("f{i}.md"), format!("# file {i}\n")))
        .collect();
    let files_refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let fx = setup_repo_with_base(&files_refs);

    // Build inputs: 19 no-op writes + one real change on f7.md.
    let inputs: Vec<WriteFileInput> = files
        .iter()
        .map(|(p, original)| WriteFileInput {
            path: p.clone(),
            content: if p == "f7.md" {
                "# actually changed\n".to_string()
            } else {
                original.clone()
            },
            message: "batch commit".to_string(),
            base_revision: None,
        })
        .collect();

    let results = commit_files(&fx.repo_root, inputs).unwrap();

    assert_eq!(
        results.len(),
        1,
        "expected exactly one changed result, got {}: {:?}",
        results.len(),
        results.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
    assert_eq!(results[0].path, "f7.md");
}

#[test]
fn test_commit_files_uses_shared_message() {
    let fx = setup_repo_with_base(&[("a.md", "# a\n")]);

    let inputs = vec![
        WriteFileInput {
            path: "a.md".into(),
            content: "# a modified\n".into(),
            message: "Refactor note A".into(),
            base_revision: None,
        },
        WriteFileInput {
            path: "b.md".into(),
            content: "# b new\n".into(),
            message: "Refactor note A".into(),
            base_revision: None,
        },
    ];

    let results = commit_files(&fx.repo_root, inputs).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(head_message(&fx.repo_root).trim(), "Refactor note A");
}

#[test]
fn test_commit_files_fallback_message_describes_categories() {
    // Base has a.md and b.md. We modify a, delete b, add c. No shared
    // message across inputs → fallback kicks in.
    let fx = setup_repo_with_base(&[("a.md", "# a\n"), ("b.md", "# b\n")]);
    // Actually delete b.md from the working tree so the index picks it up.
    std::fs::remove_file(fx.repo_root.join("b.md")).unwrap();
    let repo = Repository::open(&fx.repo_root).unwrap();
    repo.index()
        .unwrap()
        .remove_path(std::path::Path::new("b.md"))
        .unwrap();
    repo.index().unwrap().write().unwrap();
    drop(repo);

    let inputs = vec![
        WriteFileInput {
            path: "a.md".into(),
            content: "# a modified\n".into(),
            message: "msg-one".into(),
            base_revision: None,
        },
        WriteFileInput {
            path: "c.md".into(),
            content: "# c new\n".into(),
            message: "msg-two".into(),
            base_revision: None,
        },
    ];

    commit_files(&fx.repo_root, inputs).unwrap();
    let message = head_message(&fx.repo_root);
    assert!(
        message.contains("via CommitBook"),
        "message should use fallback: {message}"
    );
    assert!(
        message.contains("add") || message.contains("modify") || message.contains("delete"),
        "expected category words in fallback: {message}"
    );
    assert!(
        !message.contains("Update") || !message.contains("files"),
        "fallback should not be the old 'Update N files' format: {message}"
    );
}

#[test]
fn test_commit_files_empty_inputs_is_noop() {
    let fx = setup_repo_with_base(&[("a.md", "# a\n")]);
    let before = fx.base_sha.clone();

    let results = commit_files(&fx.repo_root, vec![]).unwrap();
    assert!(results.is_empty());

    let repo = Repository::open(&fx.repo_root).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.id().to_string(), before, "HEAD should not advance");
}
