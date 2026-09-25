use super::*;
use crate::git::test_support::{
    clone_second_workdir, commit_and_push_from, setup_repo_with_bare_remote, RepoFixture,
};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Fake {
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl ConflictResolver for Fake {
    fn name(&self) -> &str {
        "fake"
    }
    fn key(&self) -> &str {
        "fake"
    }
    fn is_available(&self) -> bool {
        true
    }
    async fn resolve(&self, _: &GitConflict, _: &Path) -> Result<ConflictResolution> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ConflictResolution::WriteContent("resolved\n".into()))
    }
}
fn fixture() -> RepoFixture {
    let fx = setup_repo_with_bare_remote();
    let root = fx.repo_dir.path();
    // Conflicts must stay conflicts here, so use manual mode, not `both`.
    let mut config = LocalConfig::new("notes", &fx.branch, "origin");
    config.conflicts.mode = crate::config::ConflictMode::Manual;
    LocalConfig::init(root, &config).unwrap();
    std::fs::write(root.join("note.md"), "base\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("base").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "note.md", "remote\n");
    std::fs::write(root.join("note.md"), "local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local").unwrap();
    fx.repo
        .fetch_with("origin", &fx.branch, &crate::platform::SystemCredentials)
        .unwrap();
    fx.repo.merge_fetched("origin", &fx.branch).unwrap();
    fx
}
#[tokio::test]
async fn proposals_survive_restart_and_block_even_after_setting_disabled() {
    let fx = fixture();
    let root = fx.repo_dir.path();
    let lock = RepoLock::acquire(root).unwrap();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    let before = std::fs::read(root.join("note.md")).unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    assert!(prepare_locked(root, true, Some(&fake), &lock)
        .await
        .unwrap());
    assert_eq!(before, std::fs::read(root.join("note.md")).unwrap());
    assert_eq!(index, std::fs::read(root.join(".git/index")).unwrap());
    assert!(prepare_locked(root, true, Some(&fake), &lock)
        .await
        .unwrap());
    assert!(prepare_locked(root, false, Some(&fake), &lock)
        .await
        .unwrap());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    let view = list(root).unwrap().remove(0);
    assert!(view.proposal.is_some());
    assert!(!view.proposal_stale);
    let input = ResolutionInput {
        proposal_version: view.proposal_version.clone(),
        path: view.path,
        revision: view.revision,
        action: "accept".into(),
        content: None,
    };
    apply_locked(root, &fx.branch, &input, &lock).unwrap();
    assert!(!fx.repo.merge_in_progress());
    assert!(list(root).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(root.join("note.md")).unwrap(),
        "resolved\n"
    );
    let raw = git2::Repository::open(root).unwrap();
    assert_eq!(
        raw.head().unwrap().peel_to_commit().unwrap().parent_count(),
        2
    );
    assert_ne!(
        raw.head().unwrap().target(),
        raw.find_reference(&format!("refs/remotes/origin/{}", fx.branch))
            .unwrap()
            .target()
    );
}
#[tokio::test]
async fn rejected_proposals_are_not_regenerated_and_stale_edits_are_rejected() {
    let fx = fixture();
    let root = fx.repo_dir.path();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    {
        let lock = RepoLock::acquire(root).unwrap();
        prepare_locked(root, true, Some(&fake), &lock)
            .await
            .unwrap();
    }
    let view = list(root).unwrap().remove(0);
    let input = ResolutionInput {
        proposal_version: view.proposal_version.clone(),
        path: view.path.clone(),
        revision: view.revision.clone(),
        action: "reject".into(),
        content: None,
    };
    proposal_action(root, &input).await.unwrap();
    {
        let lock = RepoLock::acquire(root).unwrap();
        prepare_locked(root, true, Some(&fake), &lock)
            .await
            .unwrap();
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(list(root).unwrap()[0].proposal.as_ref().unwrap().rejected);
    std::fs::write(root.join("note.md"), "user editing\n").unwrap();
    let stale = ResolutionInput {
        action: "take_local".into(),
        ..input
    };
    assert!(apply(root, &fx.branch, &stale)
        .unwrap_err()
        .downcast_ref::<ReviewError>()
        .is_some());
    assert_eq!(
        std::fs::read_to_string(root.join("note.md")).unwrap(),
        "user editing\n"
    );
    let view = list(root).unwrap().remove(0);
    let lock = RepoLock::acquire(root).unwrap();
    propose_locked(root, &view.path, &view.revision, &fake, &lock)
        .await
        .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    let input = ResolutionInput {
        proposal_version: view.proposal_version.clone(),
        path: view.path,
        revision: view.revision,
        action: "manual_edit".into(),
        content: Some("my resolution\n".into()),
    };
    apply_locked(root, &fx.branch, &input, &lock).unwrap();
    assert!(load(root).unwrap().proposals.is_empty());
}
#[test]
fn manual_actions_require_current_branch_valid_text_and_lock() {
    let fx = fixture();
    let root = fx.repo_dir.path();
    let view = list(root).unwrap().remove(0);
    let mut input = ResolutionInput {
        proposal_version: view.proposal_version.clone(),
        path: view.path,
        revision: view.revision,
        action: "manual_edit".into(),
        content: Some("<<<<<<< unresolved\n".into()),
    };
    assert!(apply(root, "wrong", &input).is_err());
    assert!(apply(root, &fx.branch, &input).is_err());
    {
        let _lock = RepoLock::acquire(root).unwrap();
        assert!(apply(root, &fx.branch, &input).is_err());
    }
    input.action = "take_remote".into();
    apply(root, &fx.branch, &input).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("note.md")).unwrap(),
        "remote\n"
    );
}

#[tokio::test]
async fn regenerated_identical_proposal_invalidates_old_accept_and_reject() {
    let fx = fixture();
    let root = fx.repo_dir.path();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    let view = list(root).unwrap().remove(0);
    {
        let lock = RepoLock::acquire(root).unwrap();
        propose_locked(root, &view.path, &view.revision, &fake, &lock)
            .await
            .unwrap();
    }
    let old = list(root).unwrap().remove(0);
    {
        let lock = RepoLock::acquire(root).unwrap();
        propose_locked(root, &view.path, &view.revision, &fake, &lock)
            .await
            .unwrap();
    }
    let new = list(root).unwrap().remove(0);
    assert_ne!(old.proposal_version, new.proposal_version);
    for action in ["accept", "reject"] {
        let input = ResolutionInput {
            path: old.path.clone(),
            revision: old.revision.clone(),
            proposal_version: old.proposal_version.clone(),
            action: action.into(),
            content: None,
        };
        assert!(matches!(
            proposal_action(root, &input)
                .await
                .unwrap_err()
                .downcast_ref::<ReviewError>(),
            Some(ReviewError::Stale)
        ));
    }
    assert!(fx.repo.merge_in_progress());
    let input = ResolutionInput {
        path: new.path,
        revision: new.revision,
        proposal_version: new.proposal_version,
        action: "accept".into(),
        content: None,
    };
    proposal_action(root, &input).await.unwrap();
    assert!(!fx.repo.merge_in_progress());
}

#[tokio::test]
async fn sync_review_pauses_without_writing_resolution_or_pushing() {
    let fx = fixture();
    let root = fx.repo_dir.path();
    let fake = Fake {
        calls: AtomicUsize::new(0),
    };
    let logger = crate::logger::FileLogger::new(root, crate::config::LogKeep::Days(30)).unwrap();
    let mut options = crate::sync::scheduler::SyncOptions::new("origin", &fx.branch, true);
    options.review_ai_resolutions = true;
    let original = std::fs::read(root.join("note.md")).unwrap();
    let head = fx.repo.rev_parse("HEAD").unwrap();
    for enabled in [true, true, false] {
        options.review_ai_resolutions = enabled;
        let outcome = crate::sync::scheduler::sync_with_resolver(
            root,
            &options,
            Some(&fake),
            &crate::platform::SystemCredentials,
            &logger,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome.manual_conflicts, 1);
        assert_eq!(outcome.pushed, 0);
        assert_eq!(outcome.conflicts_resolved, 0);
        assert_eq!(std::fs::read(root.join("note.md")).unwrap(), original);
        assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    let state =
        crate::state::sync_state::SyncState::load(&LocalConfig::commitbook_dir(root)).unwrap();
    assert!(state.last_attempt_at.is_some());
    assert!(state.last_sync_at.is_none());
    assert_eq!(state.last_error_stage.as_deref(), Some("review"));
    let error = state.last_error.unwrap();
    assert!(error.contains("await review"), "{error}");
    assert!(error.contains("note.md"), "{error}");
}

struct StateFailure;
#[async_trait::async_trait]
impl ConflictResolver for StateFailure {
    fn name(&self) -> &str {
        "failure"
    }
    fn key(&self) -> &str {
        "failure"
    }
    fn is_available(&self) -> bool {
        true
    }
    async fn resolve(&self, _: &GitConflict, root: &Path) -> Result<ConflictResolution> {
        let path = LocalConfig::local_dir(root).join("state.toml");
        std::fs::remove_file(&path)?;
        std::fs::create_dir(path)?;
        anyhow::bail!("original resolver failure")
    }
}
#[tokio::test]
async fn state_persistence_failure_retains_original_operation_error() {
    let fx = fixture();
    let root = fx.repo_dir.path();
    let logger = crate::logger::FileLogger::new(root, crate::config::LogKeep::Days(30)).unwrap();
    let result = crate::sync::scheduler::sync_with_resolver(
        root,
        &crate::sync::scheduler::SyncOptions::new("origin", &fx.branch, true),
        Some(&StateFailure),
        &crate::platform::SystemCredentials,
        &logger,
        None,
    )
    .await;
    let message = format!("{:#}", result.unwrap_err());
    assert!(message.contains("original resolver failure"), "{message}");
    assert!(message.contains("persistence"), "{message}");
    assert!(fx.repo.merge_in_progress());
}
