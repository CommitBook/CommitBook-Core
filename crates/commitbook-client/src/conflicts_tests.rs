use commitbook_engine::git::{ConflictSide, GitConflict};
use git2::Oid;

use super::*;

fn side(content: &[u8]) -> ConflictSide {
    ConflictSide {
        oid: Oid::ZERO_SHA1,
        mode: 0o100644,
        content: content.to_vec(),
    }
}

#[test]
fn structured_summaries_preserve_deleted_and_binary_sides() {
    let modify_delete = GitConflict {
        path: "note.md".to_string(),
        ancestor: Some(side(b"base\n")),
        local: Some(side(b"local\n")),
        remote: None,
    };
    assert_eq!(modify_delete.classification(), "modify_delete");
    assert_eq!(modify_delete.local_text(), Some("local\n"));
    assert_eq!(modify_delete.remote_text(), None);

    let binary = GitConflict {
        path: "blob.md".to_string(),
        ancestor: None,
        local: Some(side(b"left\0bytes")),
        remote: Some(side(b"right\0bytes")),
    };
    assert!(binary.is_binary_or_special());
    assert_eq!(binary.classification(), "binary");
}

#[test]
fn manual_resolution_refuses_wrong_branch_before_touching_index() {
    let root = tempfile::tempdir().unwrap();
    let clone = root.path().join("owner__notes");
    std::fs::create_dir(&clone).unwrap();
    let mut init = git2::RepositoryInitOptions::new();
    init.initial_head("main");
    let repository = git2::Repository::init_opts(&clone, &init).unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    drop(config);
    git2::Repository::open(&clone)
        .unwrap()
        .remote("origin", "https://github.com/owner/notes.git")
        .unwrap();
    commitbook_engine::commitbooks::init_dot_commitbook(
        &clone,
        "Notes",
        "main",
        "origin",
        None,
        commitbook_engine::config::Auth::Pat,
    )
    .unwrap();
    std::fs::write(clone.join("base.md"), "base\n").unwrap();
    let repo = GitRepo::open(&clone).unwrap();
    repo.stage_all().unwrap();
    repo.commit("base").unwrap();
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("other", &head, false).unwrap();
    drop(head);
    repository.set_head("refs/heads/other").unwrap();
    repository.checkout_head(None).unwrap();
    let index_before = std::fs::read(repository.path().join("index")).unwrap();

    let error = resolve_conflict(
        root.path(),
        &ResolveConflictInput {
            commitbook_id: "owner/notes".to_string(),
            conflict_id: "base.md".to_string(),
            resolution_type: "delete".to_string(),
            manual_content: None,
            revision: None,
            proposal_version: None,
        },
    )
    .unwrap_err();
    assert!(matches!(error, CommitBookError::InvalidInput { .. }));
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index_before
    );
    assert_eq!(
        std::fs::read_to_string(clone.join("base.md")).unwrap(),
        "base\n"
    );

    repository.set_head("refs/heads/main").unwrap();
    repository.checkout_head(None).unwrap();
    let error = resolve_conflict(
        root.path(),
        &ResolveConflictInput {
            commitbook_id: "owner/notes".to_string(),
            conflict_id: "base.md".to_string(),
            resolution_type: "delete".to_string(),
            manual_content: None,
            revision: None,
            proposal_version: None,
        },
    )
    .unwrap_err();
    assert!(matches!(error, CommitBookError::MergeError { .. }));
}
