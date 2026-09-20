use super::*;
use crate::git::operations::{push_attempts, set_push_failpoint, PushFailpoint};
use crate::git::test_support::{setup_repo_with_bare_remote, RepoFixture};
use crate::platform::SystemCredentials;

const MESSAGE: &str = "Initialize CommitBook";

/// Working repo with a bare remote and uncommitted `.CommitBook` metadata.
fn seeded() -> RepoFixture {
    let fx = setup_repo_with_bare_remote();
    let mut config = LocalConfig::new("0 * * * *");
    config.git.branch = fx.branch.clone();
    config.git.remote = "origin".to_string();
    config.save(fx.repo_dir.path()).unwrap();
    LocalConfig::ensure_gitignore(fx.repo_dir.path()).unwrap();
    fx
}

fn remote_tip(fx: &RepoFixture) -> Option<git2::Oid> {
    git2::Repository::open_bare(fx.remote_dir.path())
        .unwrap()
        .refname_to_id(&format!("refs/heads/{}", fx.branch))
        .ok()
}

fn state_of(fx: &RepoFixture) -> SyncState {
    SyncState::load(&LocalConfig::commitbook_dir(fx.repo_dir.path())).unwrap()
}

fn publish(fx: &RepoFixture, auto_push: bool) -> Result<Publication, PublicationError> {
    publish_metadata(
        &fx.repo,
        "origin",
        &fx.branch,
        MESSAGE,
        auto_push,
        &SystemCredentials,
    )
}

#[test]
fn first_publish_commits_pushes_and_clears_pending() {
    let fx = seeded();
    let before = fx.repo.rev_parse("HEAD").unwrap();

    assert_eq!(publish(&fx, true).unwrap(), Publication::Pushed);

    let head = fx.repo.rev_parse("HEAD").unwrap();
    assert_ne!(head, before);
    assert_eq!(fx.repo.rev_parse("HEAD~1").unwrap(), before);
    assert!(fx
        .repo
        .show_file_at_ref("HEAD", ".CommitBook/config.toml")
        .is_ok());
    assert!(fx
        .repo
        .show_file_at_ref("HEAD", ".CommitBook/.gitignore")
        .is_ok());
    assert_eq!(remote_tip(&fx).unwrap().to_string(), head);
    assert!(state_of(&fx).pending_init_push.is_none());
}

#[test]
fn no_op_when_nothing_to_commit_and_nothing_pending_does_not_touch_remote() {
    let fx = seeded();
    publish(&fx, true).unwrap();
    let head = fx.repo.rev_parse("HEAD").unwrap();
    git2::Repository::open(fx.repo_dir.path())
        .unwrap()
        .remote_set_url("origin", "file:///definitely/missing/commitbook.git")
        .unwrap();
    let attempts = push_attempts();

    assert_eq!(publish(&fx, true).unwrap(), Publication::Unchanged);

    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert_eq!(push_attempts(), attempts);
    assert!(state_of(&fx).pending_init_push.is_none());
}

#[test]
fn failed_push_preserves_pending_and_retry_succeeds() {
    let fx = seeded();
    let remote_before = remote_tip(&fx);
    set_push_failpoint(PushFailpoint::Auth);

    let error = publish(&fx, true).unwrap_err();
    assert!(matches!(error, PublicationError::Push(_)), "{error}");
    assert!(error.to_string().contains("local commit remains intact"));

    let head = fx.repo.rev_parse("HEAD").unwrap();
    let pending = state_of(&fx).pending_init_push.expect("pending recorded");
    assert_eq!(pending.commit_oid, head);
    assert_eq!(pending.remote, "origin");
    assert_eq!(pending.branch, fx.branch);
    assert_eq!(remote_tip(&fx), remote_before);

    assert_eq!(publish(&fx, true).unwrap(), Publication::Pushed);
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert_eq!(remote_tip(&fx).unwrap().to_string(), head);
    assert!(state_of(&fx).pending_init_push.is_none());
}

#[test]
fn pending_with_advanced_head_is_rejected() {
    let fx = seeded();
    set_push_failpoint(PushFailpoint::Auth);
    publish(&fx, true).unwrap_err();
    let pending_before = state_of(&fx).pending_init_push.clone().unwrap();
    std::fs::write(fx.repo_dir.path().join("later.md"), "later\n").unwrap();
    fx.repo.stage_paths(&["later.md".to_string()]).unwrap();
    fx.repo.commit("user edit").unwrap();
    let attempts = push_attempts();

    let error = publish(&fx, true).unwrap_err();

    assert!(matches!(error, PublicationError::Commit(_)), "{error}");
    assert!(error.to_string().contains("run sync to reconcile"));
    assert_eq!(state_of(&fx).pending_init_push.unwrap(), pending_before);
    assert_eq!(push_attempts(), attempts);
}

#[test]
fn pending_with_different_target_is_rejected() {
    let fx = seeded();
    set_push_failpoint(PushFailpoint::Auth);
    publish(&fx, true).unwrap_err();
    let pending_before = state_of(&fx).pending_init_push.clone().unwrap();
    let attempts = push_attempts();

    let error = publish_metadata(
        &fx.repo,
        "upstream",
        &fx.branch,
        MESSAGE,
        true,
        &SystemCredentials,
    )
    .unwrap_err();

    assert!(matches!(error, PublicationError::Commit(_)), "{error}");
    assert_eq!(state_of(&fx).pending_init_push.unwrap(), pending_before);
    assert_eq!(push_attempts(), attempts);
}

#[test]
fn auto_push_disabled_records_pending_without_pushing() {
    let fx = seeded();
    let remote_before = remote_tip(&fx);
    git2::Repository::open(fx.repo_dir.path())
        .unwrap()
        .remote_set_url("origin", "file:///definitely/missing/commitbook.git")
        .unwrap();
    let attempts = push_attempts();

    assert_eq!(publish(&fx, false).unwrap(), Publication::Deferred);

    let head = fx.repo.rev_parse("HEAD").unwrap();
    assert_eq!(state_of(&fx).pending_init_push.unwrap().commit_oid, head);
    assert_eq!(push_attempts(), attempts);
    // Still deferred on repeat: no new commit, no push.
    assert_eq!(publish(&fx, false).unwrap(), Publication::Deferred);
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert_eq!(push_attempts(), attempts);

    git2::Repository::open(fx.repo_dir.path())
        .unwrap()
        .remote_set_url(
            "origin",
            &format!("file://{}", fx.remote_dir.path().display()),
        )
        .unwrap();
    assert_eq!(publish(&fx, true).unwrap(), Publication::Pushed);
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head);
    assert_ne!(remote_tip(&fx), remote_before);
    assert_eq!(remote_tip(&fx).unwrap().to_string(), head);
    assert!(state_of(&fx).pending_init_push.is_none());
}

#[test]
fn clear_published_clears_only_when_pending_is_contained_in_tip() {
    let fx = seeded();
    publish(&fx, true).unwrap();
    let head = fx.repo.rev_parse("HEAD").unwrap();
    let parent = fx.repo.rev_parse("HEAD~1").unwrap();
    let directory = LocalConfig::commitbook_dir(fx.repo_dir.path());
    let pending = PendingInitPush {
        commit_oid: head.clone(),
        remote: "origin".to_string(),
        branch: fx.branch.clone(),
    };
    let write_pending = || {
        let mut state = SyncState::load(&directory).unwrap();
        state.pending_init_push = Some(pending.clone());
        state.save(&directory).unwrap();
    };

    write_pending();
    clear_published(&fx.repo, "origin", &fx.branch, &parent).unwrap();
    assert!(state_of(&fx).pending_init_push.is_some());

    clear_published(&fx.repo, "upstream", &fx.branch, &head).unwrap();
    assert!(state_of(&fx).pending_init_push.is_some());

    clear_published(&fx.repo, "origin", &fx.branch, &head).unwrap();
    assert!(state_of(&fx).pending_init_push.is_none());
}
