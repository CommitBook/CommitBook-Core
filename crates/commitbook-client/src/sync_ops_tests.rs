use super::*;
use crate::types::{AiConflictCallbackResult, AiConflictResolution, AiConflictResolutionAction};

struct ContentCallback;

impl ConflictResolverCallback for ContentCallback {
    fn resolve(
        &self,
        request: AiConflictRequest,
        continuation: Arc<ConflictResolutionContinuation>,
    ) {
        assert_eq!(request.ancestor_content.as_deref(), Some("base\n"));
        assert_eq!(request.local_content.as_deref(), Some("local\n"));
        assert_eq!(request.remote_content.as_deref(), Some("remote\n"));
        std::thread::spawn(move || {
            continuation
                .complete(AiConflictCallbackResult {
                    resolution: Some(AiConflictResolution {
                        action: AiConflictResolutionAction::WriteContent,
                        content: Some("resolved\n".to_string()),
                    }),
                    error_message: None,
                })
                .unwrap();
        });
    }
}

fn text_conflict() -> GitConflict {
    let side = |content: &str| commitbook_engine::git::ConflictSide {
        oid: git2::Oid::zero(),
        mode: 0o100644,
        content: content.as_bytes().to_vec(),
    };
    GitConflict {
        path: "note.md".to_string(),
        ancestor: Some(side("base\n")),
        local: Some(side("local\n")),
        remote: Some(side("remote\n")),
    }
}

#[test]
fn host_callback_receives_structured_sides() {
    let resolver = HostConflictResolver {
        commitbook_id: "owner/repo".to_string(),
        callback: Arc::new(ContentCallback),
    };
    let resolution = crate::runtime::runtime()
        .block_on(resolver.resolve(&text_conflict(), Path::new("/tmp")))
        .unwrap();
    assert_eq!(
        resolution,
        ConflictResolution::WriteContent("resolved\n".to_string())
    );
}

struct InvalidDeleteCallback;

impl ConflictResolverCallback for InvalidDeleteCallback {
    fn resolve(
        &self,
        _request: AiConflictRequest,
        continuation: Arc<ConflictResolutionContinuation>,
    ) {
        continuation
            .complete(AiConflictCallbackResult {
                resolution: Some(AiConflictResolution {
                    action: AiConflictResolutionAction::DeleteFile,
                    content: Some("must be absent".to_string()),
                }),
                error_message: None,
            })
            .unwrap();
    }
}

#[test]
fn host_callback_rejects_inconsistent_delete_response() {
    let resolver = HostConflictResolver {
        commitbook_id: "owner/repo".to_string(),
        callback: Arc::new(InvalidDeleteCallback),
    };
    assert!(crate::runtime::runtime()
        .block_on(resolver.resolve(&text_conflict(), Path::new("/tmp")))
        .is_err());
}

#[derive(Clone)]
struct FixedCallback(AiConflictCallbackResult);

impl ConflictResolverCallback for FixedCallback {
    fn resolve(
        &self,
        _request: AiConflictRequest,
        continuation: Arc<ConflictResolutionContinuation>,
    ) {
        continuation.complete(self.0.clone()).unwrap();
    }
}

fn run_fixed(
    result: AiConflictCallbackResult,
    conflict: &GitConflict,
) -> AnyResult<ConflictResolution> {
    let resolver = HostConflictResolver {
        commitbook_id: "owner/repo".to_string(),
        callback: Arc::new(FixedCallback(result)),
    };
    crate::runtime::runtime().block_on(resolver.resolve(conflict, Path::new("/tmp")))
}

#[test]
fn host_callback_accepts_explicit_deletion() {
    let resolution = run_fixed(
        AiConflictCallbackResult {
            resolution: Some(AiConflictResolution {
                action: AiConflictResolutionAction::DeleteFile,
                content: None,
            }),
            error_message: None,
        },
        &text_conflict(),
    )
    .unwrap();
    assert_eq!(resolution, ConflictResolution::DeleteFile);
}

#[test]
fn host_callback_surfaces_explicit_failure() {
    let error = run_fixed(
        AiConflictCallbackResult {
            resolution: None,
            error_message: Some("provider unavailable".to_string()),
        },
        &text_conflict(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("provider unavailable"));
}

#[test]
fn host_callback_rejects_unresolved_markers() {
    let result = run_fixed(
        AiConflictCallbackResult {
            resolution: Some(AiConflictResolution {
                action: AiConflictResolutionAction::WriteContent,
                content: Some("<<<<<<< local\ntext\n>>>>>>> remote\n".to_string()),
            }),
            error_message: None,
        },
        &text_conflict(),
    );
    assert!(result.is_err());
}

struct MustNotRunCallback;

impl ConflictResolverCallback for MustNotRunCallback {
    fn resolve(
        &self,
        _request: AiConflictRequest,
        _continuation: Arc<ConflictResolutionContinuation>,
    ) {
        panic!("binary conflict must not invoke host callback");
    }
}

#[test]
fn binary_conflict_never_invokes_host_callback() {
    let mut conflict = text_conflict();
    conflict.local.as_mut().unwrap().content = b"binary\0content".to_vec();
    let resolver = HostConflictResolver {
        commitbook_id: "owner/repo".to_string(),
        callback: Arc::new(MustNotRunCallback),
    };
    assert!(crate::runtime::runtime()
        .block_on(resolver.resolve(&conflict, Path::new("/tmp")))
        .is_err());
}

#[test]
fn continuation_can_only_complete_once() {
    let (sender, _receiver) = tokio::sync::oneshot::channel();
    let continuation = ConflictResolutionContinuation::new(sender);
    let result = AiConflictCallbackResult {
        resolution: None,
        error_message: Some("failure".to_string()),
    };
    continuation.complete(result.clone()).unwrap();
    assert!(continuation.complete(result).is_err());
}

struct BlockingCallback;

impl ConflictResolverCallback for BlockingCallback {
    fn resolve(
        &self,
        _request: AiConflictRequest,
        _continuation: Arc<ConflictResolutionContinuation>,
    ) {
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

#[test]
fn blocking_foreign_callback_is_covered_by_timeout() {
    let resolver = HostConflictResolver {
        commitbook_id: "owner/repo".to_string(),
        callback: Arc::new(BlockingCallback),
    };
    let started = std::time::Instant::now();
    let error = crate::runtime::runtime()
        .block_on(resolver.resolve(&text_conflict(), Path::new("/tmp")))
        .unwrap_err();
    assert!(error.to_string().contains("timed out"));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

struct PanickingCallback;

impl ConflictResolverCallback for PanickingCallback {
    fn resolve(
        &self,
        _request: AiConflictRequest,
        _continuation: Arc<ConflictResolutionContinuation>,
    ) {
        panic!("foreign callback panic");
    }
}

#[test]
fn panicking_foreign_callback_becomes_resolver_failure() {
    let resolver = HostConflictResolver {
        commitbook_id: "owner/repo".to_string(),
        callback: Arc::new(PanickingCallback),
    };
    let error = crate::runtime::runtime()
        .block_on(resolver.resolve(&text_conflict(), Path::new("/tmp")))
        .unwrap_err();
    assert!(error.to_string().contains("foreign callback panic"));
}

fn managed_sync_fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    std::path::PathBuf,
    String,
) {
    let root = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote.path()).unwrap();
    let clone = root.path().join("owner__repo");
    std::fs::create_dir(&clone).unwrap();
    let repository = git2::Repository::init(&clone).unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    drop(config);
    commitbook_engine::commitbooks::init_dot_commitbook(
        &clone, "Repo", "owner", "repo", "main", "github", "pat",
    )
    .unwrap();
    std::fs::write(clone.join("shared.md"), "base\n").unwrap();
    let repo = commitbook_engine::git::GitRepo::open(&clone).unwrap();
    repo.stage_all().unwrap();
    repo.commit("base").unwrap();
    let branch = repo.current_branch().unwrap();

    let repository = git2::Repository::open(&clone).unwrap();
    repository
        .remote("origin", remote.path().to_str().unwrap())
        .unwrap();
    let mut local_config = LocalConfig::load(&clone).unwrap();
    local_config.git.branch = branch.clone();
    local_config.git.remote = "origin".to_string();
    local_config.save(&clone).unwrap();
    repo.commit_selected_paths(&[".CommitBook/config.toml"], "configure branch")
        .unwrap();
    repo.push_with("origin", &branch, &TokenCredentials::new("unused"))
        .unwrap();
    (root, remote, clone, branch)
}

#[test]
fn ai_mode_without_callback_allows_conflict_free_sync() {
    let (root, _remote, _clone, _branch) = managed_sync_fixture();
    let outcome = sync_one_commitbook(
        root.path(),
        "owner/repo",
        SyncMode::AiResolve,
        "unused",
        None,
    )
    .unwrap();
    assert_eq!(outcome.manual_conflicts, 0);
    assert!(!outcome
        .errors
        .iter()
        .any(|error| error.contains("did not register")));
}

#[test]
fn sync_lock_contention_does_not_rewrite_legacy_config_during_lookup() {
    let (root, _remote, clone, branch) = managed_sync_fixture();
    let original = format!(
        r#"schedule = "hourly"
created_at = "now"

[git]
auto_push = true
branch = "{branch}"
remote = "origin"

[commitbook]
name = "Repo"
owner = "owner"
repo = "repo"
provider = "github"
mode = "pat"
"#
    );
    std::fs::write(clone.join(".CommitBook/config.toml"), &original).unwrap();
    let _lock = commitbook_engine::state::RepoLock::acquire(&clone).unwrap();

    let error = sync_one_commitbook(root.path(), "owner/repo", SyncMode::Manual, "unused", None)
        .unwrap_err();
    assert!(matches!(error, CommitBookError::MergeError { .. }));
    assert_eq!(
        std::fs::read_to_string(clone.join(".CommitBook/config.toml")).unwrap(),
        original
    );
}

#[test]
fn ai_mode_can_recover_preserved_merge_after_callback_is_configured() {
    let (root, remote, clone, branch) = managed_sync_fixture();
    let local_repo = commitbook_engine::git::GitRepo::open(&clone).unwrap();
    std::fs::write(clone.join("shared.md"), "local\n").unwrap();
    local_repo.stage_all().unwrap();
    local_repo.commit("local edit").unwrap();

    let other = tempfile::tempdir().unwrap();
    let repository = git2::build::RepoBuilder::new()
        .branch(&branch)
        .clone(remote.path().to_str().unwrap(), other.path())
        .unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Remote").unwrap();
    config.set_str("user.email", "remote@example.com").unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    drop(config);
    std::fs::write(other.path().join("shared.md"), "remote\n").unwrap();
    let other_repo = commitbook_engine::git::GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote edit").unwrap();
    other_repo
        .push_with("origin", &branch, &TokenCredentials::new("unused"))
        .unwrap();

    let outcome = sync_one_commitbook(
        root.path(),
        "owner/repo",
        SyncMode::AiResolve,
        "unused",
        None,
    )
    .unwrap();
    assert_eq!(outcome.manual_conflicts, 1);
    assert!(outcome
        .errors
        .iter()
        .any(|error| error.contains("did not register")));
    let local_repo = commitbook_engine::git::GitRepo::open(&clone).unwrap();
    assert!(local_repo.merge_in_progress());
    assert_eq!(
        local_repo.list_conflicted_paths().unwrap(),
        vec!["shared.md".to_string()]
    );

    let callback = FixedCallback(AiConflictCallbackResult {
        resolution: Some(AiConflictResolution {
            action: AiConflictResolutionAction::WriteContent,
            content: Some("resolved on phone\n".to_string()),
        }),
        error_message: None,
    });
    let recovered = sync_one_commitbook(
        root.path(),
        "owner/repo",
        SyncMode::AiResolve,
        "unused",
        Some(Arc::new(callback)),
    )
    .unwrap();
    assert_eq!(recovered.manual_conflicts, 0);
    assert_eq!(recovered.conflicts_resolved, 1);
    assert!(recovered.errors.is_empty(), "{recovered:?}");
    assert!(recovered.pushed >= 1);
    assert!(!local_repo.merge_in_progress());
    assert_eq!(
        std::fs::read_to_string(clone.join("shared.md")).unwrap(),
        "resolved on phone\n"
    );

    let verification = tempfile::tempdir().unwrap();
    git2::build::RepoBuilder::new()
        .branch(&branch)
        .clone(remote.path().to_str().unwrap(), verification.path())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(verification.path().join("shared.md")).unwrap(),
        "resolved on phone\n"
    );
}
