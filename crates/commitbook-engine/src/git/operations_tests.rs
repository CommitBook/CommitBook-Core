use super::*;
use crate::git::test_support::{
    clone_second_workdir, commit_and_push_from, setup_repo_with_bare_remote, setup_repo_with_base,
};
use std::fs;

fn create_temp_repo() -> (tempfile::TempDir, Repository) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();

    // Set user config so commits work
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test User").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    // Override any inherited commit.gpgsign so tests don't require a signing key.
    config.set_bool("commit.gpgsign", false).unwrap();

    (tmp, repo)
}

// --- ChangesSummary tests ---

#[test]
fn test_changes_summary_empty() {
    let s = ChangesSummary::default();
    assert!(s.is_empty());
    assert_eq!(s.total(), 0);
}

#[test]
fn test_changes_summary_total() {
    let s = ChangesSummary {
        new_files: vec!["a.rs".into()],
        modified_files: vec!["b.rs".into(), "c.rs".into()],
        deleted_files: vec![],
    };
    assert_eq!(s.total(), 3);
    assert!(!s.is_empty());
}

#[test]
fn test_push_rejection_classifies_callback_non_fast_forward() {
    let error = push_rejection_error("refs/heads/main: non-fast-forward");
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<git2::Error>()
            .is_some_and(|error| error.code() == git2::ErrorCode::NotFastForward)
    }));

    let rejected = push_rejection_error("refs/heads/main: pre-receive hook declined");
    assert!(!rejected
        .chain()
        .any(|cause| cause.downcast_ref::<git2::Error>().is_some()));
}

#[test]
fn test_changes_summary_text_empty() {
    let s = ChangesSummary::default();
    assert_eq!(s.to_summary_text(), "no changes");
}

#[test]
fn test_changes_summary_text_mixed() {
    let s = ChangesSummary {
        new_files: vec!["a.rs".into()],
        modified_files: vec!["b.rs".into()],
        deleted_files: vec!["c.rs".into()],
    };
    let text = s.to_summary_text();
    assert!(text.contains("1 new"));
    assert!(text.contains("1 modified"));
    assert!(text.contains("1 deleted"));
}

#[test]
fn test_changes_detail_text() {
    let s = ChangesSummary {
        new_files: vec!["main.rs".into()],
        modified_files: vec![],
        deleted_files: vec![],
    };
    let detail = s.to_detail_text();
    assert!(detail.contains("main.rs"));
    assert!(detail.contains("1 new file(s)"));
}

#[test]
fn test_changes_detail_text_empty() {
    let s = ChangesSummary::default();
    assert_eq!(s.to_detail_text(), "No changes");
}

// --- GitRepo tests ---

#[test]
fn test_is_repo_true() {
    let (tmp, _repo) = create_temp_repo();
    assert!(GitRepo::is_repo(tmp.path()));
}

#[test]
fn test_is_repo_false() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!GitRepo::is_repo(tmp.path()));
}

#[test]
fn test_open_repo() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path());
    assert!(git_repo.is_ok());
}

#[test]
fn test_has_remote_false() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();
    assert!(!git_repo.has_remote());
}

#[test]
fn test_current_branch_initial() {
    let (tmp, _repo) = create_temp_repo();
    // Need at least one commit for HEAD to exist
    let git_repo = GitRepo::open(tmp.path()).unwrap();
    fs::write(tmp.path().join("init.txt"), "hello").unwrap();
    git_repo.stage_all().unwrap();
    git_repo.commit("initial").unwrap();

    let branch = git_repo.current_branch().unwrap();
    // git init creates "main" or "master" depending on config
    assert!(!branch.is_empty());
}

#[test]
fn test_stage_and_commit_lifecycle() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();

    // Create a file
    fs::write(tmp.path().join("test.txt"), "content").unwrap();

    // Check changes
    let changes = git_repo.changes_summary().unwrap();
    assert!(!changes.is_empty());
    assert_eq!(changes.new_files.len(), 1);

    // Stage all
    git_repo.stage_all().unwrap();

    // Commit
    let hash = git_repo.commit("test commit").unwrap();
    assert_eq!(hash.len(), 7);

    // After commit, no changes
    let changes = git_repo.changes_summary().unwrap();
    assert!(changes.is_empty());
}

#[test]
fn test_commit_does_not_overwrite_an_external_ref_advance() {
    let fx = setup_repo_with_base(&[("notes.md", "base\n")]);
    let raw = git2::Repository::open(&fx.repo_root).unwrap();
    let head = raw.head().unwrap().peel_to_commit().unwrap();
    let tree = head.tree().unwrap();
    let signature = raw.signature().unwrap();
    let external_oid = raw
        .commit(
            None,
            &signature,
            &signature,
            "external advance",
            &tree,
            &[&head],
        )
        .unwrap();
    drop(tree);
    drop(head);
    drop(raw);

    fs::write(fx.repo_root.join("notes.md"), "local\n").unwrap();
    fx.repo().stage_all().unwrap();
    set_commit_publish_advance_to(external_oid);
    let error = fx.repo().commit("local commit").unwrap_err().to_string();

    assert!(error.contains("refusing to overwrite external Git work"));
    assert_eq!(
        fx.repo().rev_parse("HEAD").unwrap(),
        external_oid.to_string()
    );
}

#[test]
fn test_commit_does_not_publish_after_same_tip_branch_switch() {
    let fx = setup_repo_with_base(&[("notes.md", "base\n")]);
    let raw = git2::Repository::open(&fx.repo_root).unwrap();
    let original_branch = fx.repo().current_branch().unwrap();
    let original_oid = raw.head().unwrap().target().unwrap();
    let original_commit = raw.find_commit(original_oid).unwrap();
    raw.branch("other", &original_commit, false).unwrap();
    drop(original_commit);
    drop(raw);

    fs::write(fx.repo_root.join("notes.md"), "local\n").unwrap();
    fx.repo().stage_all().unwrap();
    set_commit_publish_switch_head_to("refs/heads/other");
    let error = fx.repo().commit("local commit").unwrap_err().to_string();

    assert!(error.contains("HEAD changed"));
    assert_eq!(fx.repo().current_branch().unwrap(), "other");
    assert_eq!(
        fx.repo()
            .rev_parse(&format!("refs/heads/{original_branch}"))
            .unwrap(),
        original_oid.to_string()
    );
    assert_eq!(
        fx.repo().rev_parse("refs/heads/other").unwrap(),
        original_oid.to_string()
    );
}

#[test]
fn test_changes_summary_modified() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();

    // Initial commit
    let file_path = tmp.path().join("file.txt");
    fs::write(&file_path, "v1").unwrap();
    git_repo.stage_all().unwrap();
    git_repo.commit("init").unwrap();

    // Modify file
    fs::write(&file_path, "v2").unwrap();
    let changes = git_repo.changes_summary().unwrap();
    assert_eq!(changes.modified_files.len(), 1);
}

#[test]
fn test_diff_summary_handles_clean_repository() {
    let fx = setup_repo_with_base(&[("notes.md", "clean\n")]);

    // A clean diff still travels through git2::Buf formatting. Keep exercising
    // that zero-change path so upgrades cannot reintroduce the former null-buffer issue.
    assert_eq!(
        fx.repo().diff_summary().unwrap(),
        "0 files changed, 0 insertions(+), 0 deletions(-)"
    );
}

#[test]
fn test_compiled_libgit2_supports_https_and_ssh_transports() {
    let version = git2::Version::get();

    assert!(
        version.https(),
        "libgit2 was compiled without HTTPS support"
    );
    assert!(version.ssh(), "libgit2 was compiled without SSH support");
}

#[test]
fn test_diff_summary_lists_each_file_once_after_concurrent_edit() {
    // Simulates the race condition that produced commit messages like
    // "The staged change adds an entry; the unstaged ...":
    // user edits a file between stage_all() and the diff_summary() shell-out.
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();

    // Initial commit so HEAD exists.
    let file_path = tmp.path().join("notes.md");
    fs::write(&file_path, "v1\n").unwrap();
    git_repo.stage_all().unwrap();
    git_repo.commit("init").unwrap();

    // First edit -> stage it.
    fs::write(&file_path, "v1\nfirst edit\n").unwrap();
    git_repo.stage_all().unwrap();

    // Concurrent edit lands AFTER staging but BEFORE diff_summary().
    fs::write(&file_path, "v1\nfirst edit\nconcurrent edit\n").unwrap();

    let summary = git_repo.diff_summary().unwrap();
    let occurrences = summary.matches("notes.md").count();
    assert_eq!(
        occurrences, 1,
        "expected notes.md to appear exactly once, got summary:\n{}",
        summary
    );
}

#[test]
fn test_diff_summary_works_before_first_commit() {
    // Pre-HEAD: `git diff HEAD` errors, but we should still get the staged diff.
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();
    fs::write(tmp.path().join("brand_new.md"), "hello\n").unwrap();
    git_repo.stage_all().unwrap();

    let summary = git_repo.diff_summary().unwrap();
    assert!(
        summary.contains("brand_new.md"),
        "expected brand_new.md in summary, got:\n{}",
        summary
    );
}

// --- New helper tests: fetch / merge_ff_only / ahead_behind / show_file_at_ref / ls_tree_files / rev_parse ---

#[test]
fn test_rev_parse_resolves_head() {
    let fx = setup_repo_with_bare_remote();
    let sha = fx.repo.rev_parse("HEAD").unwrap();
    assert_eq!(sha.len(), 40);
}

#[test]
fn test_rev_parse_resolves_remote_ref() {
    let fx = setup_repo_with_bare_remote();
    let head_sha = fx.repo.rev_parse("HEAD").unwrap();
    let remote_sha = fx.repo.rev_parse(&format!("origin/{}", fx.branch)).unwrap();
    assert_eq!(head_sha, remote_sha);
}

#[test]
fn test_ls_tree_files_lists_initial_commit() {
    let fx = setup_repo_with_bare_remote();
    let files = fx.repo.ls_tree_files("HEAD").unwrap();
    assert_eq!(files, vec!["init.md".to_string()]);
}

#[test]
fn test_show_file_at_ref_reads_committed_content() {
    let fx = setup_repo_with_bare_remote();
    let content = fx.repo.show_file_at_ref("HEAD", "init.md").unwrap();
    assert_eq!(content, "# init\n");
}

#[test]
fn test_show_file_at_ref_ignores_uncommitted_edits() {
    let fx = setup_repo_with_bare_remote();
    // Dirty the working tree after the commit.
    fs::write(fx.repo_dir.path().join("init.md"), "uncommitted\n").unwrap();

    let content = fx.repo.show_file_at_ref("HEAD", "init.md").unwrap();
    assert_eq!(content, "# init\n");
}

#[test]
fn test_fetch_advances_remote_ref() {
    let fx = setup_repo_with_bare_remote();

    let before = fx.repo.rev_parse(&format!("origin/{}", fx.branch)).unwrap();

    // A second workdir pushes a new commit.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");

    // Fetch in the original repo picks up the new commit.
    fx.repo.fetch("origin", &fx.branch).unwrap();

    let after = fx.repo.rev_parse(&format!("origin/{}", fx.branch)).unwrap();
    assert_ne!(before, after);
}

#[test]
fn test_ahead_behind_zero_when_in_sync() {
    let fx = setup_repo_with_bare_remote();
    let (ahead, behind) = fx
        .repo
        .ahead_behind("HEAD", &format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!((ahead, behind), (0, 0));
}

#[test]
fn test_ahead_behind_reports_remote_only_commits() {
    let fx = setup_repo_with_bare_remote();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");
    fx.repo.fetch("origin", &fx.branch).unwrap();

    let (ahead, behind) = fx
        .repo
        .ahead_behind("HEAD", &format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!(ahead, 0);
    assert_eq!(behind, 1);
}

#[test]
fn test_ahead_behind_reports_divergence() {
    let fx = setup_repo_with_bare_remote();

    // Local makes a commit that is not pushed.
    fs::write(fx.repo_dir.path().join("local.md"), "local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local only").unwrap();

    // Another workdir makes a different commit on remote.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");
    fx.repo.fetch("origin", &fx.branch).unwrap();

    let (ahead, behind) = fx
        .repo
        .ahead_behind("HEAD", &format!("origin/{}", fx.branch))
        .unwrap();
    assert_eq!(ahead, 1);
    assert_eq!(behind, 1);
}

#[test]
fn test_merge_ff_only_noop_when_equal() {
    let fx = setup_repo_with_bare_remote();
    let before = fx.repo.rev_parse("HEAD").unwrap();
    let advanced = fx
        .repo
        .merge_ff_only(&format!("origin/{}", fx.branch))
        .unwrap();
    assert!(advanced);
    let after = fx.repo.rev_parse("HEAD").unwrap();
    assert_eq!(before, after);
}

#[test]
fn test_merge_ff_only_fast_forwards_when_linear() {
    let fx = setup_repo_with_bare_remote();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");
    fx.repo.fetch("origin", &fx.branch).unwrap();
    let remote_sha = fx.repo.rev_parse(&format!("origin/{}", fx.branch)).unwrap();

    let advanced = fx
        .repo
        .merge_ff_only(&format!("origin/{}", fx.branch))
        .unwrap();
    assert!(advanced);
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), remote_sha);
    assert!(!fx.repo.has_dirty_changes().unwrap());
}

#[test]
fn test_merge_ff_only_rejects_divergent() {
    let fx = setup_repo_with_bare_remote();

    fs::write(fx.repo_dir.path().join("local.md"), "local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local only").unwrap();
    let local_sha = fx.repo.rev_parse("HEAD").unwrap();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");
    fx.repo.fetch("origin", &fx.branch).unwrap();

    let advanced = fx
        .repo
        .merge_ff_only(&format!("origin/{}", fx.branch))
        .unwrap();
    assert!(!advanced);
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), local_sha);
}

#[test]
fn test_changes_summary_deleted() {
    let (tmp, _repo) = create_temp_repo();
    let git_repo = GitRepo::open(tmp.path()).unwrap();

    // Initial commit
    let file_path = tmp.path().join("doomed.txt");
    fs::write(&file_path, "bye").unwrap();
    git_repo.stage_all().unwrap();
    git_repo.commit("init").unwrap();

    // Delete file
    fs::remove_file(&file_path).unwrap();
    let changes = git_repo.changes_summary().unwrap();
    assert_eq!(changes.deleted_files.len(), 1);
}

#[test]
fn test_has_dirty_changes_includes_hidden_and_non_markdown_files() {
    let fx = setup_repo_with_base(&[]);
    fs::create_dir_all(fx.repo_root.join(".hidden")).unwrap();
    fs::write(fx.repo_root.join(".hidden/data.bin"), b"data").unwrap();
    assert!(fx.repo().has_dirty_changes().unwrap());
}

#[test]
fn test_has_dirty_changes_excludes_gitignored_files() {
    let fx = setup_repo_with_base(&[(".gitignore", "ignored/\n")]);
    fs::create_dir_all(fx.repo_root.join("ignored")).unwrap();
    fs::write(fx.repo_root.join("ignored/secret.txt"), "secret").unwrap();
    assert!(!fx.repo().has_dirty_changes().unwrap());
}

#[test]
fn test_commit_selected_paths_preserves_unrelated_staged_changes() {
    let fx = setup_repo_with_base(&[("note.md", "old\n")]);
    fs::write(fx.repo_root.join("note.md"), "staged user edit\n").unwrap();
    fx.repo().stage_paths(&["note.md".to_string()]).unwrap();
    fs::create_dir_all(fx.repo_root.join(".CommitBook")).unwrap();
    fs::write(
        fx.repo_root.join(".CommitBook/config.toml"),
        "config_version = \"1\"\n",
    )
    .unwrap();
    fs::write(fx.repo_root.join(".CommitBook/.gitignore"), "/local/\n").unwrap();

    let repo = fx.repo();
    let committed = repo
        .commit_selected_paths(
            &[".CommitBook/config.toml", ".CommitBook/.gitignore"],
            "Initialize CommitBook metadata",
        )
        .unwrap();
    assert!(committed.is_some());
    assert_eq!(repo.show_file_at_ref("HEAD", "note.md").unwrap(), "old\n");
    assert_eq!(
        repo.show_file_at_ref("HEAD", ".CommitBook/.gitignore")
            .unwrap(),
        "/local/\n"
    );
    let status = git2::Repository::open(&fx.repo_root)
        .unwrap()
        .status_file(Path::new("note.md"))
        .unwrap();
    assert!(status.is_index_modified());
}

#[test]
fn test_commit_selected_paths_clears_stale_staged_blob_when_worktree_matches_head() {
    let fx = setup_repo_with_base(&[(".CommitBook/config.toml", "version = 1\n")]);
    let config_path = fx.repo_root.join(".CommitBook/config.toml");
    fs::write(&config_path, "staged stale value\n").unwrap();
    fx.repo()
        .stage_paths(&[".CommitBook/config.toml".to_string()])
        .unwrap();
    fs::write(&config_path, "version = 1\n").unwrap();
    let branch = fx.repo().current_branch().unwrap();

    let committed = fx
        .repo()
        .commit_selected_paths_on_branch(
            &[".CommitBook/config.toml"],
            "Repair CommitBook metadata",
            &branch,
        )
        .unwrap();

    assert!(committed.is_none());
    let status = git2::Repository::open(&fx.repo_root)
        .unwrap()
        .status_file(Path::new(".CommitBook/config.toml"))
        .unwrap();
    assert_eq!(status, git2::Status::CURRENT);
}

#[test]
fn test_commit_selected_paths_clears_staged_deletion_when_worktree_matches_head() {
    let fx = setup_repo_with_base(&[(".CommitBook/config.toml", "version = 1\n")]);
    let config_path = fx.repo_root.join(".CommitBook/config.toml");
    fs::remove_file(&config_path).unwrap();
    fx.repo().stage_all().unwrap();
    fs::write(&config_path, "version = 1\n").unwrap();
    let branch = fx.repo().current_branch().unwrap();

    let committed = fx
        .repo()
        .commit_selected_paths_on_branch(
            &[".CommitBook/config.toml"],
            "Repair CommitBook metadata",
            &branch,
        )
        .unwrap();

    assert!(committed.is_none());
    let status = git2::Repository::open(&fx.repo_root)
        .unwrap()
        .status_file(Path::new(".CommitBook/config.toml"))
        .unwrap();
    assert_eq!(status, git2::Status::CURRENT);
}

#[test]
fn test_commit_selected_paths_on_branch_rejects_different_head() {
    let fx = setup_repo_with_base(&[("note.md", "old\n")]);
    let configured_branch = fx.repo().current_branch().unwrap();
    let repo = git2::Repository::open(&fx.repo_root).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("other", &head, false).unwrap();
    repo.set_head("refs/heads/other").unwrap();
    drop(head);
    drop(repo);
    fs::create_dir_all(fx.repo_root.join(".CommitBook")).unwrap();
    fs::write(
        fx.repo_root.join(".CommitBook/config.toml"),
        "config_version = \"1\"\n",
    )
    .unwrap();

    let head_before = fx.repo().rev_parse("HEAD").unwrap();
    let error = fx
        .repo()
        .commit_selected_paths_on_branch(
            &[".CommitBook/config.toml"],
            "Initialize CommitBook metadata",
            &configured_branch,
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("does not match configured branch"));
    assert_eq!(fx.repo().rev_parse("HEAD").unwrap(), head_before);
    assert!(fx.repo().has_dirty_changes().unwrap());
}

#[test]
fn test_merge_from_remote_clean_when_in_sync() {
    let fx = setup_repo_with_bare_remote();
    let outcome = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    assert_eq!(outcome, MergeOutcome::Clean);
}

#[test]
fn test_merge_from_remote_fast_forwards_when_behind() {
    let fx = setup_repo_with_bare_remote();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");

    let outcome = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    assert_eq!(outcome, MergeOutcome::Clean);
    // Working tree should now contain the pulled file.
    assert!(fx.repo_dir.path().join("new.md").exists());
}

#[test]
fn test_merge_from_remote_creates_merge_commit_on_diverge() {
    let fx = setup_repo_with_bare_remote();

    // Local: a committed change to a unique file.
    fs::write(fx.repo_dir.path().join("local-only.md"), "# local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local commit").unwrap();
    let local_sha = fx.repo.rev_parse("HEAD").unwrap();

    // Remote: a different unique file pushed via a second clone.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote-only.md", "# remote\n");

    let outcome = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    assert_eq!(outcome, MergeOutcome::Clean);

    // After merge: HEAD has both files and is a merge commit (2 parents).
    assert!(fx.repo_dir.path().join("local-only.md").exists());
    assert!(fx.repo_dir.path().join("remote-only.md").exists());
    let new_sha = fx.repo.rev_parse("HEAD").unwrap();
    assert_ne!(
        new_sha, local_sha,
        "HEAD should advance to the merge commit"
    );
}

#[test]
fn test_merge_from_remote_returns_conflicts_when_paths_overlap() {
    let fx = setup_repo_with_bare_remote();

    // Establish a shared committed file.
    fs::write(fx.repo_dir.path().join("shared.md"), "line A\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    // Local: commit a divergent change.
    fs::write(fx.repo_dir.path().join("shared.md"), "line A LOCAL\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local edit").unwrap();

    // Remote: commit a different divergent change to the same line.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "shared.md", "line A REMOTE\n");

    let outcome = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    match outcome {
        MergeOutcome::Conflicts(paths) => {
            assert_eq!(paths, vec!["shared.md".to_string()]);
        }
        MergeOutcome::Clean => panic!("expected conflicts"),
    }

    let conflicted = fx.repo.list_conflicted_paths().unwrap();
    assert_eq!(conflicted, vec!["shared.md".to_string()]);
}

#[test]
fn test_list_conflicted_paths_empty_when_clean() {
    let fx = setup_repo_with_bare_remote();
    let paths = fx.repo.list_conflicted_paths().unwrap();
    assert!(paths.is_empty());
}

#[test]
fn test_finalize_merge_commit_after_resolution() {
    let fx = setup_repo_with_bare_remote();

    // Shared committed file.
    fs::write(fx.repo_dir.path().join("shared.md"), "line A\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    // Local + remote divergent edits.
    fs::write(fx.repo_dir.path().join("shared.md"), "line A LOCAL\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local edit").unwrap();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "shared.md", "line A REMOTE\n");

    // Trigger conflict.
    let outcome = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    match outcome {
        MergeOutcome::Conflicts(paths) => assert_eq!(paths, vec!["shared.md".to_string()]),
        MergeOutcome::Clean => panic!("expected conflicts"),
    }

    // Simulate resolution: rewrite content + stage.
    fs::write(fx.repo_dir.path().join("shared.md"), "line A RESOLVED\n").unwrap();
    fx.repo.stage_paths(&["shared.md".to_string()]).unwrap();

    // Finalize the merge commit.
    fx.repo.finalize_merge_commit(None).unwrap();

    // Index is clean.
    assert!(fx.repo.list_conflicted_paths().unwrap().is_empty());
    // HEAD is a merge commit (2 parents).
    let head_oid = fx.repo.rev_parse("HEAD").unwrap();
    let repo = git2::Repository::open(fx.repo_dir.path()).unwrap();
    let head_commit = repo
        .find_commit(git2::Oid::from_str(&head_oid).unwrap())
        .unwrap();
    assert_eq!(head_commit.parent_count(), 2);
}

#[test]
fn test_finalize_merge_commit_requires_merge_in_progress() {
    let fx = setup_repo_with_base(&[("notes.md", "base\n")]);
    fs::write(fx.repo_root.join("notes.md"), "staged edit\n").unwrap();
    fx.repo().stage_paths(&["notes.md".to_string()]).unwrap();
    let head_before = fx.repo().rev_parse("HEAD").unwrap();

    let error = fx
        .repo()
        .finalize_merge_commit(None)
        .unwrap_err()
        .to_string();

    assert!(error.contains("no merge is in progress"));
    assert_eq!(fx.repo().rev_parse("HEAD").unwrap(), head_before);
}

#[test]
fn test_merge_abort_resets_conflicted_state() {
    let fx = setup_repo_with_bare_remote();

    // Shared committed file.
    fs::write(fx.repo_dir.path().join("shared.md"), "line A\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add shared").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    // Diverge on the same path.
    fs::write(fx.repo_dir.path().join("shared.md"), "line A LOCAL\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("local edit").unwrap();
    let head_before = fx.repo.rev_parse("HEAD").unwrap();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "shared.md", "line A REMOTE\n");

    let _ = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    assert!(!fx.repo.list_conflicted_paths().unwrap().is_empty());

    fx.repo.merge_abort().unwrap();

    // No conflicts, HEAD unchanged, working tree restored.
    assert!(fx.repo.list_conflicted_paths().unwrap().is_empty());
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
    assert_eq!(
        fs::read_to_string(fx.repo_dir.path().join("shared.md")).unwrap(),
        "line A LOCAL\n"
    );
}

#[test]
fn test_fast_forward_refuses_to_overwrite_dirty_tracked_file() {
    let fx = setup_repo_with_bare_remote();

    // Commit + push a non-markdown tracked file so it exists in history.
    fs::write(fx.repo_dir.path().join("data.txt"), "base\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add data").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    // Remote advances by modifying that same file: a pure fast-forward for us.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "data.txt", "remote change\n");

    // Locally dirty the same tracked file (uncommitted). The scheduler only
    // commits dirty markdown, so a dirty .txt reaches the fast-forward checkout.
    fs::write(
        fx.repo_dir.path().join("data.txt"),
        "uncommitted local change\n",
    )
    .unwrap();

    // The fast-forward must refuse rather than silently discard the local edit.
    let result = fx.repo.merge_from_remote("origin", &fx.branch);
    assert!(
        result.is_err(),
        "fast-forward must refuse to overwrite a dirty tracked file, got {result:?}"
    );
    assert_eq!(
        fs::read_to_string(fx.repo_dir.path().join("data.txt")).unwrap(),
        "uncommitted local change\n",
        "the local edit must be preserved"
    );
}

#[test]
fn test_fast_forward_preserves_unrelated_local_deletion() {
    let fx = setup_repo_with_bare_remote();
    fs::write(fx.repo_dir.path().join("keep.txt"), "tracked\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add tracked file").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "remote\n");
    fs::remove_file(fx.repo_dir.path().join("keep.txt")).unwrap();

    let outcome = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    assert_eq!(outcome, MergeOutcome::Clean);
    assert!(!fx.repo_dir.path().join("keep.txt").exists());
    assert!(fx.repo_dir.path().join("remote.md").exists());
}

#[test]
fn test_fast_forward_blocks_untracked_remote_addition_collision() {
    let fx = setup_repo_with_bare_remote();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "collision.txt", "remote\n");
    fs::write(fx.repo_dir.path().join("collision.txt"), "local\n").unwrap();

    let error = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap_err()
        .to_string();
    assert!(error.contains("collision.txt"));
    assert_eq!(
        fs::read_to_string(fx.repo_dir.path().join("collision.txt")).unwrap(),
        "local\n"
    );
}

#[test]
fn test_fast_forward_blocks_ignored_remote_addition_collision() {
    let fx = setup_repo_with_bare_remote();
    fs::write(fx.repo_dir.path().join(".gitignore"), "collision.txt\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("ignore collision path").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    fs::write(other.path().join("collision.txt"), "remote\n").unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo
        .stage_paths(&["collision.txt".to_string()])
        .unwrap();
    other_repo.commit("force tracked ignored path").unwrap();
    other_repo.push("origin", &fx.branch).unwrap();

    fs::write(fx.repo_dir.path().join("collision.txt"), "local ignored\n").unwrap();
    let head_before = fx.repo.rev_parse("HEAD").unwrap();
    let error = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap_err()
        .to_string();

    assert!(error.contains("collision.txt"));
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
    assert_eq!(
        fs::read_to_string(fx.repo_dir.path().join("collision.txt")).unwrap(),
        "local ignored\n"
    );
}

#[test]
fn test_fast_forward_blocks_staged_new_remote_addition_collision() {
    let fx = setup_repo_with_bare_remote();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "collision.txt", "remote\n");
    fs::write(fx.repo_dir.path().join("collision.txt"), "local\n").unwrap();
    fx.repo.stage_paths(&["collision.txt".to_string()]).unwrap();

    let error = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap_err()
        .to_string();
    assert!(error.contains("collision.txt"));
}

#[test]
fn test_fast_forward_blocking_parent_does_not_advance_head() {
    let fx = setup_repo_with_bare_remote();
    let head_before = fx.repo.rev_parse("HEAD").unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "folder/remote.md", "remote\n");
    fs::write(fx.repo_dir.path().join("folder"), "blocking file\n").unwrap();

    assert!(fx.repo.merge_from_remote("origin", &fx.branch).is_err());
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
    assert_eq!(
        fs::read_to_string(fx.repo_dir.path().join("folder")).unwrap(),
        "blocking file\n"
    );
}

#[test]
fn test_fast_forward_checkout_failure_precedes_ref_advancement() {
    let fx = setup_repo_with_bare_remote();
    let head_before = fx.repo.rev_parse("HEAD").unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "remote\n");
    set_fast_forward_failpoint(FastForwardFailpoint::BeforeCheckout);

    let error = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap_err()
        .to_string();

    assert!(error.contains("BeforeCheckout"));
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
    assert!(!fx.repo_dir.path().join("remote.md").exists());
}

#[test]
fn test_fast_forward_ref_publication_failure_rolls_back_checkout() {
    let fx = setup_repo_with_bare_remote();
    let head_before = fx.repo.rev_parse("HEAD").unwrap();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "remote\n");
    set_fast_forward_failpoint(FastForwardFailpoint::BeforeRefPublication);

    let error = fx.repo.merge_from_remote("origin", &fx.branch).unwrap_err();

    assert!(format!("{error:#}").contains("BeforeRefPublication"));
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), head_before);
    assert!(!fx.repo_dir.path().join("remote.md").exists());
    assert!(!fx.repo.has_dirty_changes().unwrap());
}

#[test]
fn test_fast_forward_does_not_overwrite_external_branch_advance() {
    let fx = setup_repo_with_bare_remote();
    let raw = git2::Repository::open(fx.repo_dir.path()).unwrap();
    let head = raw.head().unwrap().peel_to_commit().unwrap();
    let tree = head.tree().unwrap();
    let signature = raw.signature().unwrap();
    let external_oid = raw
        .commit(
            None,
            &signature,
            &signature,
            "external local commit",
            &tree,
            &[&head],
        )
        .unwrap();
    drop(tree);
    drop(head);
    drop(raw);

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "remote\n");
    set_fast_forward_external_advance_to(external_oid);

    let error = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap_err()
        .to_string();

    assert!(error.contains("refusing to overwrite external Git work"));
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), external_oid.to_string());
    assert!(!fx.repo_dir.path().join("remote.md").exists());
}

#[test]
fn test_fast_forward_does_not_update_same_tip_switched_branch() {
    let fx = setup_repo_with_bare_remote();
    let raw = git2::Repository::open(fx.repo_dir.path()).unwrap();
    let original_oid = raw.head().unwrap().target().unwrap();
    let original_commit = raw.find_commit(original_oid).unwrap();
    raw.branch("other", &original_commit, false).unwrap();
    drop(original_commit);
    drop(raw);

    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "remote\n");
    set_fast_forward_switch_head_to("refs/heads/other");

    let error = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap_err()
        .to_string();

    assert!(error.contains("HEAD changed"));
    assert_eq!(fx.repo.current_branch().unwrap(), "other");
    assert_eq!(
        fx.repo
            .rev_parse(&format!("refs/heads/{}", fx.branch))
            .unwrap(),
        original_oid.to_string()
    );
    assert_eq!(
        fx.repo.rev_parse("refs/heads/other").unwrap(),
        original_oid.to_string()
    );
    assert!(!fx.repo_dir.path().join("remote.md").exists());
}

#[test]
fn test_fast_forward_rechecks_ancestry_from_latest_head() {
    let fx = setup_repo_with_bare_remote();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "remote.md", "remote\n");
    fx.repo.fetch("origin", &fx.branch).unwrap();
    let remote_oid =
        git2::Oid::from_str(&fx.repo.rev_parse(&format!("origin/{}", fx.branch)).unwrap()).unwrap();

    fs::write(fx.repo_dir.path().join("local.md"), "local\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("external local advance").unwrap();
    let local_oid = fx.repo.rev_parse("HEAD").unwrap();

    let expected_head = fx.repo.capture_head_expectation().unwrap();
    let error = fx
        .repo
        .fast_forward_to(remote_oid, &expected_head)
        .unwrap_err()
        .to_string();

    assert!(error.contains("no longer an ancestor"));
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), local_oid);
    assert!(fx.repo_dir.path().join("local.md").exists());
    assert!(!fx.repo_dir.path().join("remote.md").exists());
}

#[test]
fn test_last_commit_touching_skips_merge_equal_to_parent() {
    let fx = setup_repo_with_bare_remote();

    // Commit + push f.md = v1.
    fs::write(fx.repo_dir.path().join("f.md"), "v1\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("add f v1").unwrap();
    fx.repo.push("origin", &fx.branch).unwrap();

    // Remote advances by touching a different file; f.md stays v1 there.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "other.md", "# other\n");

    // Locally edit f.md -> v2 and commit; capture that SHA.
    fs::write(fx.repo_dir.path().join("f.md"), "v2\n").unwrap();
    fx.repo.stage_all().unwrap();
    fx.repo.commit("edit f to v2").unwrap();
    let edit_sha = fx.repo.rev_parse("HEAD").unwrap();

    // Diverged merge creates a merge commit whose f.md (v2) equals the local
    // parent (base v1, ours v2, theirs v1 merges cleanly to v2).
    let outcome = fx.repo.merge_from_remote("origin", &fx.branch).unwrap();
    assert_eq!(outcome, MergeOutcome::Clean);
    let merge_sha = fx.repo.rev_parse("HEAD").unwrap();
    assert_ne!(merge_sha, edit_sha, "expected a merge commit");

    // The edit, not the merge commit, is the last commit that touched f.md.
    // (This is the regression guard for the `.all` fix; `.any` would return
    // the merge commit here.)
    assert_eq!(
        fx.repo.last_commit_touching("f.md").unwrap(),
        Some(edit_sha)
    );
}
