use super::*;
use crate::git::test_support::{
    clone_second_workdir, commit_and_push_from, setup_repo_with_base, setup_repo_with_bare_remote,
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
    let remote_sha = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
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

    let before = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();

    // A second workdir pushes a new commit.
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");

    // Fetch in the original repo picks up the new commit.
    fx.repo.fetch("origin", &fx.branch).unwrap();

    let after = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();
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
    let remote_sha = fx
        .repo
        .rev_parse(&format!("origin/{}", fx.branch))
        .unwrap();

    let advanced = fx
        .repo
        .merge_ff_only(&format!("origin/{}", fx.branch))
        .unwrap();
    assert!(advanced);
    assert_eq!(fx.repo.rev_parse("HEAD").unwrap(), remote_sha);
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

// --- status_markdown tests ---

#[test]
fn test_status_markdown_clean_working_tree() {
    let fx = setup_repo_with_base(&[("notes.md", "# notes\n")]);
    let status = fx.repo().status_markdown().unwrap();
    assert!(status.is_empty());
}

#[test]
fn test_status_markdown_modified() {
    let fx = setup_repo_with_base(&[("notes.md", "# notes\n")]);
    fs::write(fx.repo_root.join("notes.md"), "# modified\n").unwrap();

    let status = fx.repo().status_markdown().unwrap();
    assert_eq!(status.modified, vec!["notes.md".to_string()]);
    assert!(status.added.is_empty());
    assert!(status.deleted.is_empty());
}

#[test]
fn test_status_markdown_added_untracked() {
    let fx = setup_repo_with_base(&[]);
    fs::write(fx.repo_root.join("new.md"), "# new\n").unwrap();

    let status = fx.repo().status_markdown().unwrap();
    assert_eq!(status.added, vec!["new.md".to_string()]);
    assert!(status.modified.is_empty());
    assert!(status.deleted.is_empty());
}

#[test]
fn test_status_markdown_deleted() {
    let fx = setup_repo_with_base(&[("notes.md", "# notes\n")]);
    fs::remove_file(fx.repo_root.join("notes.md")).unwrap();

    let status = fx.repo().status_markdown().unwrap();
    assert_eq!(status.deleted, vec!["notes.md".to_string()]);
    assert!(status.modified.is_empty());
    assert!(status.added.is_empty());
}

#[test]
fn test_status_markdown_skips_non_markdown() {
    let fx = setup_repo_with_base(&[]);
    fs::write(fx.repo_root.join("script.sh"), "#!/bin/bash\n").unwrap();
    fs::write(fx.repo_root.join("image.png"), b"fake").unwrap();
    fs::write(fx.repo_root.join("real.md"), "# real\n").unwrap();

    let status = fx.repo().status_markdown().unwrap();
    assert_eq!(status.added, vec!["real.md".to_string()]);
    assert!(!status.added.iter().any(|p| p.ends_with(".sh")));
    assert!(!status.added.iter().any(|p| p.ends_with(".png")));
}

#[test]
fn test_status_markdown_skips_hidden_dirs() {
    let fx = setup_repo_with_base(&[]);
    fs::create_dir_all(fx.repo_root.join(".hidden")).unwrap();
    fs::write(fx.repo_root.join(".hidden/secret.md"), "# secret\n").unwrap();
    fs::write(fx.repo_root.join("visible.md"), "# visible\n").unwrap();

    let status = fx.repo().status_markdown().unwrap();
    assert_eq!(status.added, vec!["visible.md".to_string()]);
    assert!(!status.added.iter().any(|p| p.contains(".hidden")));
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

// --- has_dirty_markdown / merge_from_remote / list_conflicted_paths tests ---

#[test]
fn test_has_dirty_markdown_clean_repo() {
    let fx = setup_repo_with_base(&[("notes.md", "# notes\n")]);
    assert!(!fx.repo().has_dirty_markdown().unwrap());
}

#[test]
fn test_has_dirty_markdown_modified_md() {
    let fx = setup_repo_with_base(&[("notes.md", "# notes\n")]);
    fs::write(fx.repo_root.join("notes.md"), "# modified\n").unwrap();
    assert!(fx.repo().has_dirty_markdown().unwrap());
}

#[test]
fn test_has_dirty_markdown_ignores_non_md() {
    let fx = setup_repo_with_base(&[]);
    fs::write(fx.repo_root.join("script.sh"), "#!/bin/bash\n").unwrap();
    assert!(!fx.repo().has_dirty_markdown().unwrap());
}

#[test]
fn test_merge_from_remote_clean_when_in_sync() {
    let fx = setup_repo_with_bare_remote();
    let outcome = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap();
    assert_eq!(outcome, MergeOutcome::Clean);
}

#[test]
fn test_merge_from_remote_fast_forwards_when_behind() {
    let fx = setup_repo_with_bare_remote();
    let other = clone_second_workdir(fx.remote_dir.path(), &fx.branch);
    commit_and_push_from(other.path(), &fx.branch, "new.md", "# new\n");

    let outcome = fx
        .repo
        .merge_from_remote("origin", &fx.branch)
        .unwrap();
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
    assert_ne!(new_sha, local_sha, "HEAD should advance to the merge commit");
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
