use super::*;

fn input(branch: &str) -> CommitBookInput {
    CommitBookInput {
        name: "Notes".to_string(),
        mode: "pat".to_string(),
        provider: "github".to_string(),
        owner: "owner".to_string(),
        repo: "notes".to_string(),
        branch: branch.to_string(),
        device_name: Some("Phone".to_string()),
    }
}

/// Bare remote path whose URL identifies `owner/notes`, matching `input`.
fn remote_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("owner").join("notes.git")
}

fn initialize_repo(path: &Path) -> (GitRepo, String) {
    let repository = git2::Repository::init(path).unwrap();
    let mut config = repository.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    drop(config);
    std::fs::write(path.join("base.md"), "base\n").unwrap();
    let repo = GitRepo::open(path).unwrap();
    repo.stage_all().unwrap();
    repo.commit("base").unwrap();
    let branch = repo.current_branch().unwrap();
    (repo, branch)
}

#[test]
fn initialization_repairs_metadata_pushes_and_preserves_unrelated_index() {
    let local = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let (repo, branch) = initialize_repo(local.path());
    let repository = git2::Repository::open(local.path()).unwrap();
    repository
        .remote("origin", remote_path(&remote).to_str().unwrap())
        .unwrap();
    repo.push_with("origin", &branch, &TokenCredentials::new("unused"))
        .unwrap();

    std::fs::write(local.path().join("staged.md"), "user staged\n").unwrap();
    repo.stage_paths(&["staged.md".to_string()]).unwrap();

    ensure_commitbook_initialized(
        local.path(),
        &input(&branch),
        "origin",
        &TokenCredentials::new("unused"),
    )
    .unwrap();

    let config = LocalConfig::load(local.path()).unwrap();
    assert_eq!(config.commitbook.name, "Notes");
    let (id, device) = commitbook_engine::devices::this_device(local.path())
        .unwrap()
        .unwrap();
    assert_eq!(device.name, "Phone");
    assert_eq!(device.auth, commitbook_engine::config::Auth::Pat);
    assert_eq!(
        std::fs::read_to_string(local.path().join(".CommitBook/.gitignore")).unwrap(),
        "/local/\n"
    );

    let repository = git2::Repository::open(local.path()).unwrap();
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    assert!(head
        .tree()
        .unwrap()
        .get_path(Path::new("staged.md"))
        .is_err());
    assert!(repository
        .index()
        .unwrap()
        .get_path(Path::new("staged.md"), 0)
        .is_some());
    let remote_repository = git2::Repository::open_bare(remote_path(&remote)).unwrap();
    assert_eq!(
        remote_repository
            .refname_to_id(&format!("refs/heads/{branch}"))
            .unwrap(),
        head.id()
    );
    assert!(head
        .tree()
        .unwrap()
        .get_path(Path::new(&commitbook_engine::devices::device_repo_path(
            &id
        )))
        .is_ok());

    let first_head = head.id();
    drop(head);
    ensure_commitbook_initialized(
        local.path(),
        &input(&branch),
        "origin",
        &TokenCredentials::new("unused"),
    )
    .unwrap();
    assert_eq!(
        git2::Repository::open(local.path())
            .unwrap()
            .head()
            .unwrap()
            .target(),
        Some(first_head)
    );
}

#[test]
fn existing_committed_config_is_kept_and_this_device_is_published() {
    let local = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let (repo, branch) = initialize_repo(local.path());
    git2::Repository::open(local.path())
        .unwrap()
        .remote("origin", remote_path(&remote).to_str().unwrap())
        .unwrap();

    let mut legacy = LocalConfig::new("notes", "main", "origin");
    legacy.git.branch = branch.clone();
    legacy.git.remote = "origin".to_string();
    legacy.save(local.path()).unwrap();
    LocalConfig::ensure_gitignore(local.path()).unwrap();
    repo.commit_selected_paths(
        &[".CommitBook/config.toml", ".CommitBook/.gitignore"],
        "legacy config",
    )
    .unwrap();
    repo.push_with("origin", &branch, &TokenCredentials::new("unused"))
        .unwrap();
    let legacy_head = git2::Repository::open(local.path())
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();

    // Deliberately pass another valid branch and name. The existing config
    // remains the authority; only this device's file is added.
    let mut request = input("different-branch");
    request.name = "Ignored".to_string();
    ensure_commitbook_initialized(
        local.path(),
        &request,
        "origin",
        &TokenCredentials::new("unused"),
    )
    .unwrap();

    let repaired = LocalConfig::load(local.path()).unwrap();
    assert_eq!(repaired.git.branch, branch);
    assert_eq!(repaired.commitbook.name, "notes");
    let repaired_head = git2::Repository::open(local.path())
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    assert_ne!(repaired_head, legacy_head);
    assert_eq!(
        git2::Repository::open_bare(remote_path(&remote))
            .unwrap()
            .refname_to_id(&format!("refs/heads/{branch}"))
            .unwrap(),
        repaired_head
    );
}

#[test]
fn existing_clone_without_config_infers_and_persists_sole_remote() {
    let root = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let clone = root.path().join("owner__notes");
    std::fs::create_dir(&clone).unwrap();
    let (repo, branch) = initialize_repo(&clone);
    git2::Repository::open(&clone)
        .unwrap()
        .remote("upstream", remote_path(&remote).to_str().unwrap())
        .unwrap();
    repo.push_with("upstream", &branch, &TokenCredentials::new("unused"))
        .unwrap();

    let (remote_name, _) = configured_or_inferred_target(&clone, &branch).unwrap();
    ensure_commitbook_initialized(
        &clone,
        &input(&branch),
        &remote_name,
        &TokenCredentials::new("unused"),
    )
    .unwrap();
    assert_eq!(LocalConfig::load(&clone).unwrap().git.branch, branch);
    assert_eq!(LocalConfig::load(&clone).unwrap().git.remote, "upstream");
}

#[test]
fn fresh_clone_preserves_explicit_configured_remote_name() {
    let seed = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let (seed_repo, branch) = initialize_repo(seed.path());
    git2::Repository::open(seed.path())
        .unwrap()
        .remote("upstream", remote_path(&remote).to_str().unwrap())
        .unwrap();

    let mut committed_config = LocalConfig::new("notes", "main", "origin");
    committed_config.git.branch = branch.clone();
    committed_config.git.remote = "upstream".to_string();
    committed_config.save(seed.path()).unwrap();
    LocalConfig::ensure_gitignore(seed.path()).unwrap();
    seed_repo
        .commit_selected_paths(
            &[".CommitBook/config.toml", ".CommitBook/.gitignore"],
            "seed configured remote",
        )
        .unwrap();
    seed_repo
        .push_with("upstream", &branch, &TokenCredentials::new("unused"))
        .unwrap();
    let remote_before = git2::Repository::open_bare(remote_path(&remote))
        .unwrap()
        .refname_to_id(&format!("refs/heads/{branch}"))
        .unwrap();

    // Model the libgit2 fresh-clone path without contacting GitHub: the
    // clone-created remote is `origin`, while the committed config still says
    // `upstream`.
    let root = tempfile::tempdir().unwrap();
    let clone_path = root.path().join("owner__notes");
    let mut builder = git2::build::RepoBuilder::new();
    builder.branch(&branch);
    builder
        .clone(remote_path(&remote).to_str().unwrap(), &clone_path)
        .unwrap();
    let repository = git2::Repository::open(&clone_path).unwrap();
    assert!(repository.find_remote("origin").is_ok());
    assert!(repository.find_remote("upstream").is_err());
    drop(repository);

    let _lock = RepoLock::acquire(&clone_path).unwrap();
    assert!(validate_existing_identity(&clone_path, &input(&branch)).is_err());
    let (configured_remote, configured_branch) =
        configure_fresh_clone_target(&clone_path, &branch).unwrap();
    assert_eq!(configured_remote, "upstream");
    assert_eq!(configured_branch, branch);
    ensure_commitbook_initialized(
        &clone_path,
        &input(&branch),
        &configured_remote,
        &TokenCredentials::new("unused"),
    )
    .unwrap();

    let repository = git2::Repository::open(&clone_path).unwrap();
    assert!(repository.find_remote("origin").is_err());
    assert!(repository.find_remote("upstream").is_ok());
    assert_eq!(
        repository
            .config()
            .unwrap()
            .get_string(&format!("branch.{branch}.remote"))
            .unwrap(),
        "upstream"
    );
    assert_eq!(
        LocalConfig::load(&clone_path).unwrap().git.remote,
        "upstream"
    );
    let local_head = repository.head().unwrap().target().unwrap();
    assert_ne!(local_head, remote_before);
    assert_eq!(
        git2::Repository::open_bare(remote_path(&remote))
            .unwrap()
            .refname_to_id(&format!("refs/heads/{branch}"))
            .unwrap(),
        local_head
    );
}

#[test]
fn fresh_clone_checks_out_configured_branch_and_tracks_renamed_remote() {
    let seed = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let (seed_repo, clone_branch) = initialize_repo(seed.path());
    let seed_repository = git2::Repository::open(seed.path()).unwrap();
    seed_repository
        .remote("upstream", remote_path(&remote).to_str().unwrap())
        .unwrap();

    let configured_branch = "sync-notes";
    let mut committed_config = LocalConfig::new("notes", "main", "origin");
    committed_config.git.branch = configured_branch.to_string();
    committed_config.git.remote = "upstream".to_string();
    committed_config.save(seed.path()).unwrap();
    LocalConfig::ensure_gitignore(seed.path()).unwrap();
    seed_repo
        .commit_selected_paths(
            &[".CommitBook/config.toml", ".CommitBook/.gitignore"],
            "seed configured branch",
        )
        .unwrap();
    let configured_commit = seed_repository.head().unwrap().peel_to_commit().unwrap();
    seed_repository
        .branch(configured_branch, &configured_commit, false)
        .unwrap();
    drop(configured_commit);
    seed_repo
        .push_with("upstream", &clone_branch, &TokenCredentials::new("unused"))
        .unwrap();
    seed_repo
        .push_with(
            "upstream",
            configured_branch,
            &TokenCredentials::new("unused"),
        )
        .unwrap();
    let bare_repository = git2::Repository::open_bare(remote_path(&remote)).unwrap();
    let clone_branch_before = bare_repository
        .refname_to_id(&format!("refs/heads/{clone_branch}"))
        .unwrap();
    let configured_branch_before = bare_repository
        .refname_to_id(&format!("refs/heads/{configured_branch}"))
        .unwrap();
    assert_eq!(clone_branch_before, configured_branch_before);
    drop(bare_repository);

    // Model a fresh GitHub clone: the request selects one branch, but its
    // committed configuration selects another branch and remote name.
    let root = tempfile::tempdir().unwrap();
    let clone_path = root.path().join("owner__notes");
    let mut builder = git2::build::RepoBuilder::new();
    builder.branch(&clone_branch);
    builder
        .clone(remote_path(&remote).to_str().unwrap(), &clone_path)
        .unwrap();
    assert_eq!(
        GitRepo::open(&clone_path)
            .unwrap()
            .current_branch()
            .unwrap(),
        clone_branch
    );

    let _lock = RepoLock::acquire(&clone_path).unwrap();
    let (configured_remote, recovered_branch) =
        configure_fresh_clone_target(&clone_path, &clone_branch).unwrap();
    assert_eq!(configured_remote, "upstream");
    assert_eq!(recovered_branch, configured_branch);
    assert_eq!(
        GitRepo::open(&clone_path)
            .unwrap()
            .current_branch()
            .unwrap(),
        configured_branch
    );

    let repository = git2::Repository::open(&clone_path).unwrap();
    let local_branch = repository
        .find_branch(configured_branch, git2::BranchType::Local)
        .unwrap();
    assert_eq!(
        local_branch.upstream().unwrap().get().name().unwrap(),
        "refs/remotes/upstream/sync-notes"
    );
    drop(local_branch);
    drop(repository);

    ensure_commitbook_initialized(
        &clone_path,
        &input(&clone_branch),
        &configured_remote,
        &TokenCredentials::new("unused"),
    )
    .unwrap();

    let repository = git2::Repository::open(&clone_path).unwrap();
    assert_eq!(
        repository.head().unwrap().shorthand().unwrap(),
        configured_branch
    );
    assert!(repository.find_remote("origin").is_err());
    assert!(repository.find_remote("upstream").is_ok());
    let local_head = repository.head().unwrap().target().unwrap();
    let bare_repository = git2::Repository::open_bare(remote_path(&remote)).unwrap();
    assert_eq!(
        bare_repository
            .refname_to_id(&format!("refs/heads/{clone_branch}"))
            .unwrap(),
        clone_branch_before
    );
    assert_eq!(
        bare_repository
            .refname_to_id(&format!("refs/heads/{configured_branch}"))
            .unwrap(),
        local_head
    );
    assert_ne!(local_head, configured_branch_before);
}

#[test]
fn fresh_clone_missing_configured_branch_fails_without_partial_reconfiguration() {
    let seed = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let (seed_repo, clone_branch) = initialize_repo(seed.path());
    git2::Repository::open(seed.path())
        .unwrap()
        .remote("upstream", remote_path(&remote).to_str().unwrap())
        .unwrap();

    let configured_branch = "not-pushed";
    let mut committed_config = LocalConfig::new("notes", "main", "origin");
    committed_config.git.branch = configured_branch.to_string();
    committed_config.git.remote = "upstream".to_string();
    committed_config.save(seed.path()).unwrap();
    LocalConfig::ensure_gitignore(seed.path()).unwrap();
    seed_repo
        .commit_selected_paths(
            &[".CommitBook/config.toml", ".CommitBook/.gitignore"],
            "seed missing configured branch",
        )
        .unwrap();
    seed_repo
        .push_with("upstream", &clone_branch, &TokenCredentials::new("unused"))
        .unwrap();

    let root = tempfile::tempdir().unwrap();
    let clone_path = root.path().join("owner__notes");
    let mut builder = git2::build::RepoBuilder::new();
    builder.branch(&clone_branch);
    builder
        .clone(remote_path(&remote).to_str().unwrap(), &clone_path)
        .unwrap();
    let head_before = git2::Repository::open(&clone_path)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();

    let error = configure_fresh_clone_target(&clone_path, &clone_branch).unwrap_err();
    assert!(matches!(error, CommitBookError::InvalidInput { .. }));
    let message = error.to_string();
    assert!(message.contains(configured_branch));
    assert!(message.contains("push that branch"));
    assert!(message.contains("remove the incomplete fresh clone"));
    assert!(message.contains(&clone_path.display().to_string()));

    let repository = git2::Repository::open(&clone_path).unwrap();
    assert_eq!(
        repository.head().unwrap().shorthand().unwrap(),
        clone_branch
    );
    assert_eq!(repository.head().unwrap().target(), Some(head_before));
    assert!(repository.find_remote("origin").is_ok());
    assert!(repository.find_remote("upstream").is_err());
    assert!(repository
        .find_branch(configured_branch, git2::BranchType::Local)
        .is_err());
}

#[test]
fn fresh_clone_rejects_unsafe_committed_branch_before_ref_construction() {
    let clone = tempfile::tempdir().unwrap();
    initialize_repo(clone.path());
    let mut config = LocalConfig::new("notes", "main", "origin");
    config.git.remote = "origin".to_string();
    config.git.branch = "../escape".to_string();
    config.save(clone.path()).unwrap();

    let error = configure_fresh_clone_target(clone.path(), "main").unwrap_err();
    assert!(matches!(error, CommitBookError::InvalidInput { .. }));
    assert!(error.to_string().contains("Invalid branch name"));
}

#[test]
fn existing_clone_bootstraps_metadata_to_empty_bare_remote() {
    let root = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let clone = root.path().join("owner__notes");
    std::fs::create_dir(&clone).unwrap();
    let (_repo, branch) = initialize_repo(&clone);
    git2::Repository::open(&clone)
        .unwrap()
        .remote("upstream", remote_path(&remote).to_str().unwrap())
        .unwrap();

    let (remote_name, _) = configured_or_inferred_target(&clone, &branch).unwrap();
    ensure_commitbook_initialized(
        &clone,
        &input(&branch),
        &remote_name,
        &TokenCredentials::new("unused"),
    )
    .unwrap();
    assert_eq!(LocalConfig::load(&clone).unwrap().git.branch, branch);
    let local_head = git2::Repository::open(&clone)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    assert_eq!(
        git2::Repository::open_bare(remote_path(&remote))
            .unwrap()
            .refname_to_id(&format!("refs/heads/{branch}"))
            .unwrap(),
        local_head
    );
}

#[test]
fn configless_existing_clone_rejects_zero_or_multiple_remotes() {
    let local = tempfile::tempdir().unwrap();
    let (_repo, branch) = initialize_repo(local.path());
    let zero = configured_or_inferred_target(local.path(), &branch).unwrap_err();
    assert!(matches!(zero, CommitBookError::InvalidInput { .. }));

    let repository = git2::Repository::open(local.path()).unwrap();
    repository.remote("one", "file:///tmp/one.git").unwrap();
    repository.remote("two", "file:///tmp/two.git").unwrap();
    let multiple = configured_or_inferred_target(local.path(), &branch).unwrap_err();
    assert!(matches!(multiple, CommitBookError::InvalidInput { .. }));
}

#[test]
fn failed_metadata_push_leaves_local_commit_for_retry() {
    let local = tempfile::tempdir().unwrap();
    let remote_parent = tempfile::tempdir().unwrap();
    let remote_path = remote_parent.path().join("temporarily-unavailable.git");
    let (_repo, branch) = initialize_repo(local.path());
    git2::Repository::open(local.path())
        .unwrap()
        .remote("origin", remote_path.to_str().unwrap())
        .unwrap();

    let result = ensure_commitbook_initialized(
        local.path(),
        &input(&branch),
        "origin",
        &TokenCredentials::new("unused"),
    );
    let error = result.unwrap_err();
    assert!(matches!(&error, CommitBookError::TransportError { .. }));
    assert!(error.to_string().contains("commit remains intact"));
    assert!(error.to_string().contains("run sync"));
    assert!(error.to_string().contains("retry initialization"));

    let repository = git2::Repository::open(local.path()).unwrap();
    let tree = repository.head().unwrap().peel_to_tree().unwrap();
    assert!(tree.get_path(Path::new(".CommitBook/config.toml")).is_ok());
    assert!(tree.get_path(Path::new(".CommitBook/.gitignore")).is_ok());

    // Repair the remote and retry. Metadata is already committed, so this
    // proves idempotent initialization still retries publication.
    git2::Repository::init_bare(&remote_path).unwrap();
    ensure_commitbook_initialized(
        local.path(),
        &input(&branch),
        "origin",
        &TokenCredentials::new("unused"),
    )
    .unwrap();
    assert_eq!(
        git2::Repository::open_bare(&remote_path)
            .unwrap()
            .refname_to_id(&format!("refs/heads/{branch}"))
            .unwrap(),
        repository.head().unwrap().target().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn initialization_rejects_symlinked_metadata_files_without_touching_targets() {
    use std::os::unix::fs::symlink;

    for filename in ["config.toml", ".gitignore"] {
        let local = tempfile::tempdir().unwrap();
        let (_repo, branch) = initialize_repo(local.path());
        std::fs::create_dir_all(local.path().join(".CommitBook")).unwrap();
        if filename == ".gitignore" {
            let mut config = LocalConfig::new("notes", "main", "origin");
            config.git.branch = branch.clone();
            config.save(local.path()).unwrap();
        }
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("target");
        std::fs::write(&target, "do not touch\n").unwrap();
        symlink(&target, local.path().join(".CommitBook").join(filename)).unwrap();

        let result = ensure_commitbook_initialized(
            local.path(),
            &input(&branch),
            "origin",
            &TokenCredentials::new("unused"),
        );
        assert!(result.is_err(), "{filename} symlink should be rejected");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "do not touch\n");
    }
}

#[cfg(unix)]
#[test]
fn initialization_rejects_symlinked_commitbook_directory() {
    use std::os::unix::fs::symlink;

    let local = tempfile::tempdir().unwrap();
    let (_repo, branch) = initialize_repo(local.path());
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), local.path().join(".CommitBook")).unwrap();

    assert!(ensure_commitbook_initialized(
        local.path(),
        &input(&branch),
        "origin",
        &TokenCredentials::new("unused"),
    )
    .is_err());
    assert!(!outside.path().join("config.toml").exists());
    assert!(!outside.path().join("local").exists());
}

#[test]
fn initialization_refuses_merge_in_progress_without_mutation() {
    let local = tempfile::tempdir().unwrap();
    let (_repo, branch) = initialize_repo(local.path());
    let repository = git2::Repository::open(local.path()).unwrap();
    let head = repository.head().unwrap().target().unwrap();
    std::fs::write(repository.path().join("MERGE_HEAD"), format!("{head}\n")).unwrap();

    let result = ensure_commitbook_initialized(
        local.path(),
        &input(&branch),
        "origin",
        &TokenCredentials::new("unused"),
    );
    assert!(matches!(result, Err(CommitBookError::MergeError { .. })));
    assert_eq!(repository.head().unwrap().target(), Some(head));
    assert!(!local.path().join(".CommitBook/config.toml").exists());
}

#[test]
fn initialization_refuses_branch_mismatch_before_metadata_write() {
    let local = tempfile::tempdir().unwrap();
    let (_repo, branch) = initialize_repo(local.path());
    let result = ensure_commitbook_initialized(
        local.path(),
        &input("other-branch"),
        "origin",
        &TokenCredentials::new("unused"),
    );
    assert!(matches!(result, Err(CommitBookError::InvalidInput { .. })));
    assert!(!local.path().join(".CommitBook/config.toml").exists());
    assert_eq!(
        git2::Repository::open(local.path())
            .unwrap()
            .head()
            .unwrap()
            .shorthand()
            .unwrap(),
        branch
    );
}

#[test]
fn existing_clone_summary_uses_config_name_remote_identity_and_device_auth() {
    let root = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(remote_path(&remote)).unwrap();
    git2::Repository::init_bare(remote_path(&remote)).unwrap();
    let clone = root.path().join("owner__notes");
    std::fs::create_dir(&clone).unwrap();
    let (repo, branch) = initialize_repo(&clone);
    git2::Repository::open(&clone)
        .unwrap()
        .remote("origin", remote_path(&remote).to_str().unwrap())
        .unwrap();
    commitbook_engine::commitbooks::init_dot_commitbook(
        &clone,
        "Preserved name",
        &branch,
        "origin",
        Some("Laptop"),
        commitbook_engine::config::Auth::ExistingLocalRepo,
    )
    .unwrap();
    repo.commit_selected_paths(
        &[".CommitBook/config.toml", ".CommitBook/.gitignore"],
        "metadata",
    )
    .unwrap();
    repo.push_with("origin", &branch, &TokenCredentials::new("unused"))
        .unwrap();

    let mut request = input(&branch);
    request.name = "Ignored input name".to_string();
    request.mode = "pat".to_string();
    let client =
        crate::CommitBookEngineClient::new(root.path().to_string_lossy().into_owned(), None)
            .unwrap();
    let summary = client
        .register_local_commitbook("owner__notes".into())
        .unwrap();
    assert_eq!(
        summary.commitbook_id,
        commitbook_engine::commitbooks::identity::load(&clone).unwrap()
    );
    assert_eq!(summary.owner, "owner");
    assert_eq!(summary.repo, "notes");
    assert_eq!(summary.name, "Preserved name");
    assert_eq!(summary.provider, "generic_git");
    assert_eq!(summary.mode, "existing_local_repo");
    assert_eq!(summary.branch, branch);
}

#[test]
fn existing_slug_collision_rejects_a_clone_of_another_repository() {
    let root = tempfile::tempdir().unwrap();
    let request = CommitBookInput {
        name: "Requested".to_string(),
        mode: "pat".to_string(),
        provider: "github".to_string(),
        owner: "a__b".to_string(),
        repo: "c".to_string(),
        branch: "main".to_string(),
        device_name: None,
    };
    let clone = root.path().join(slug_for(&request.owner, &request.repo));
    std::fs::create_dir(&clone).unwrap();
    let (_repo, branch) = initialize_repo(&clone);
    git2::Repository::open(&clone)
        .unwrap()
        .remote("origin", "https://github.com/a/b__c.git")
        .unwrap();
    commitbook_engine::commitbooks::init_dot_commitbook(
        &clone,
        "Different repository",
        &branch,
        "origin",
        None,
        commitbook_engine::config::Auth::Pat,
    )
    .unwrap();
    let before = std::fs::read(clone.join(".CommitBook/config.toml")).unwrap();

    let error = init_local_commitbook(root.path(), &request, "unused").unwrap_err();
    assert!(
        matches!(error, CommitBookError::InvalidInput { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("a/b__c"), "{error}");
    assert_eq!(
        std::fs::read(clone.join(".CommitBook/config.toml")).unwrap(),
        before
    );
}

#[test]
fn invalid_auth_mode_is_rejected() {
    let local = tempfile::tempdir().unwrap();
    let (_repo, branch) = initialize_repo(local.path());
    let mut request = input(&branch);
    request.mode = "password".to_string();

    let error = ensure_commitbook_initialized(
        local.path(),
        &request,
        "origin",
        &TokenCredentials::new("unused"),
    )
    .unwrap_err();
    assert!(
        matches!(error, CommitBookError::InvalidInput { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("github_app"), "{error}");
}

#[test]
fn unsupported_provider_is_rejected_before_filesystem_changes() {
    let root = tempfile::tempdir().unwrap();
    let nonexistent = root.path().join("not-created");
    let mut request = input("main");
    request.provider = "gitlab".into();
    let error = init_local_commitbook(&nonexistent, &request, "unused").unwrap_err();
    assert!(matches!(error, CommitBookError::InvalidInput { .. }));
    assert!(error.to_string().contains("only provider github"));
    assert!(!nonexistent.exists());
}

#[test]
fn same_owner_repo_on_another_host_is_not_the_requested_clone() {
    let root = tempfile::tempdir().unwrap();
    let clone = root.path().join("owner__notes");
    let repo = git2::Repository::init(&clone).unwrap();
    repo.remote("origin", "https://gitlab.com/owner/notes.git")
        .unwrap();
    let error = init_local_commitbook(root.path(), &input("main"), "unused").unwrap_err();
    assert!(matches!(error, CommitBookError::InvalidInput { .. }));
    assert!(error.to_string().contains("gitlab.com"));
    assert!(!LocalConfig::config_path(&clone).exists());
    repo.remote_set_url("origin", "git@github.com:owner/notes.git")
        .unwrap();
    validate_existing_identity(&clone, &input("main")).unwrap();
}
