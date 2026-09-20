use super::*;
use crate::git::test_support::{setup_repo_with_base, BaseRepoFixture};
fn setup() -> BaseRepoFixture {
    let fx = setup_repo_with_base(&[
        ("note.md", "original\n"),
        ("delete.txt", "delete me\n"),
        (".gitignore", ".CommitBook/local/\nignored*\n"),
    ]);
    let mut config = LocalConfig::init(&fx.repo_root, "hourly").unwrap();
    config.git.branch = fx.branch.clone();
    config.save(&fx.repo_root).unwrap();
    fx.repo().stage_all().unwrap();
    fx.repo().commit("settings").unwrap();
    fx
}
#[test]
fn preview_matches_actual_staging_without_writes() {
    let fx = setup();
    let root = &fx.repo_root;
    std::fs::write(root.join("note.md"), "staged version\n").unwrap();
    fx.repo().stage_all().unwrap();
    std::fs::write(root.join("note.md"), "original\n").unwrap();
    std::fs::remove_file(root.join("delete.txt")).unwrap();
    std::fs::write(root.join("new name.json"), "{}\n").unwrap();
    std::fs::write(root.join(".hidden"), "hidden\n").unwrap();
    std::fs::write(root.join("ignored.txt"), "ignore\n").unwrap();
    let before = snapshot(root);
    let preview = preview(root);
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    assert_eq!(
        snapshot(root),
        before,
        "preview must not write index, objects, config, logs or state"
    );
    assert!(!preview
        .entries
        .iter()
        .any(|e| e.path == "note.md" || e.path == "ignored.txt"));
    assert_eq!(preview.entries.len(), 3, "{:?}", preview.entries);
    fx.repo().stage_all().unwrap();
    let repo = git2::Repository::open(root).unwrap();
    let head = repo.head().unwrap().peel_to_tree().unwrap();
    let diff = repo.diff_tree_to_index(Some(&head), None, None).unwrap();
    let mut paths: Vec<_> = diff
        .deltas()
        .map(|d| d.new_file().path().unwrap().to_string_lossy().to_string())
        .collect();
    paths.sort();
    assert_eq!(
        preview
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<Vec<_>>(),
        paths
    );
}
#[test]
fn preview_staged_deletion_recreated_and_staged_addition_removed() {
    let fx = setup();
    std::fs::remove_file(fx.repo_root.join("note.md")).unwrap();
    std::fs::write(fx.repo_root.join("added.txt"), "added").unwrap();
    fx.repo().stage_all().unwrap();
    std::fs::write(fx.repo_root.join("note.md"), "new worktree content\n").unwrap();
    std::fs::remove_file(fx.repo_root.join("added.txt")).unwrap();
    let entries = preview_files(&fx.repo_root).unwrap();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].path, "note.md");
    assert_eq!(entries[0].change, "modified");
    assert!(entries[0].staged && entries[0].unstaged);
}
#[test]
fn status_is_read_only_and_reports_actual_head_and_unknown_remote() {
    let fx = setup();
    std::fs::remove_dir_all(LocalConfig::local_dir(&fx.repo_root)).unwrap();
    let before = snapshot(&fx.repo_root);
    let status = RepositoryStatus::read(&fx.repo_root);
    assert_eq!(status.last_commit.as_deref(), Some("settings"));
    assert!(status.head_timestamp.is_some());
    assert!(status.ahead.is_none());
    assert_eq!(status.remote_status, "Remote status unknown");
    assert!(status.diagnostics.is_empty(), "{:?}", status.diagnostics);
    assert_eq!(before, snapshot(&fx.repo_root));
}
#[test]
fn malformed_state_and_configuration_never_claim_up_to_date() {
    let fx = setup();
    std::fs::write(
        LocalConfig::local_dir(&fx.repo_root).join("state.toml"),
        "broken = [",
    )
    .unwrap();
    std::fs::write(LocalConfig::config_path(&fx.repo_root), "broken = [").unwrap();
    let s = RepositoryStatus::read(&fx.repo_root);
    assert_eq!(s.diagnostics.len(), 2);
    assert!(s.remote_status.contains("unknown"));
    assert!(s.last_commit.is_some());
}
#[test]
fn status_reports_cached_ahead_behind_and_branch_mismatch() {
    let fx = setup();
    let raw = git2::Repository::open(&fx.repo_root).unwrap();
    let base = raw.head().unwrap().target().unwrap();
    raw.reference(
        &format!("refs/remotes/origin/{}", fx.branch),
        base,
        true,
        "test",
    )
    .unwrap();
    let s = RepositoryStatus::read(&fx.repo_root);
    assert_eq!(s.ahead, Some(0));
    assert!(s.remote_status.starts_with("Up to date"));
    std::fs::write(fx.repo_root.join("note.md"), "ahead").unwrap();
    fx.repo().stage_all().unwrap();
    fx.repo().commit("ahead").unwrap();
    let s = RepositoryStatus::read(&fx.repo_root);
    assert_eq!(s.ahead, Some(1));
    assert!(s.remote_status.contains("Waiting to upload"));
    let mut config = LocalConfig::load_read_only(&fx.repo_root).unwrap();
    config.git.auto_push = false;
    config.save(&fx.repo_root).unwrap();
    assert!(RepositoryStatus::read(&fx.repo_root)
        .remote_status
        .contains("Commits kept local"));
    config.git.branch = "different".into();
    config.save(&fx.repo_root).unwrap();
    assert!(RepositoryStatus::read(&fx.repo_root)
        .diagnostics
        .iter()
        .any(|d| d.contains("branch")));
}
#[test]
fn unborn_repo_has_no_invented_commit() {
    let tmp = tempfile::tempdir().unwrap();
    git2::Repository::init(tmp.path()).unwrap();
    LocalConfig::init(tmp.path(), "hourly").unwrap();
    let s = RepositoryStatus::read(tmp.path());
    assert!(s.head_oid.is_none());
    assert!(s.last_commit.is_none());
    assert!(s.changes_total.is_some());
}
pub(crate) fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let ty = entry.file_type().unwrap();
            if ty.is_dir() {
                visit(root, &path, out);
            } else if ty.is_file() {
                out.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut out = vec![];
    visit(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn preview_keeps_tracked_ignored_changes_and_type_changes() {
    let fx = setup();
    let root = &fx.repo_root;
    std::fs::write(root.join("tracked.bin"), "base").unwrap();
    fx.repo().stage_all().unwrap();
    fx.repo().commit("tracked").unwrap();
    std::fs::write(
        root.join(".gitignore"),
        ".CommitBook/local/\nignored*\ntracked.bin\n",
    )
    .unwrap();
    std::fs::write(root.join("tracked.bin"), "changed").unwrap();
    #[cfg(unix)]
    {
        std::fs::remove_file(root.join("note.md")).unwrap();
        std::os::unix::fs::symlink("tracked.bin", root.join("note.md")).unwrap();
    }
    let entries = preview_files(root).unwrap();
    assert!(entries
        .iter()
        .any(|e| e.path == "tracked.bin" && e.change == "modified"));
    #[cfg(unix)]
    assert!(entries
        .iter()
        .any(|e| e.path == "note.md" && e.change == "type_changed"));
}

#[test]
fn preview_matches_staging_when_ignore_rules_interact_with_index() {
    for staged_addition in [true, false] {
        let fx = setup();
        let root = &fx.repo_root;
        let path = if staged_addition {
            "ignored-new.txt"
        } else {
            "note.md"
        };
        if staged_addition {
            std::fs::write(root.join(path), "new").unwrap();
            fx.repo().stage_paths(&[path.into()]).unwrap();
        } else {
            std::fs::remove_file(root.join(path)).unwrap();
            fx.repo().stage_all().unwrap();
            std::fs::write(root.join(path), "recreated").unwrap();
            std::fs::write(root.join(".gitignore"), ".CommitBook/local/\nnote.md\n").unwrap();
        }
        let preview = preview_files(root).unwrap();
        fx.repo().stage_all().unwrap();
        let raw = git2::Repository::open(root).unwrap();
        let tree = raw.head().unwrap().peel_to_tree().unwrap();
        let diff = raw.diff_tree_to_index(Some(&tree), None, None).unwrap();
        let actual = diff
            .deltas()
            .find(|d| d.new_file().path() == Some(Path::new(path)))
            .map(|d| d.status());
        let expected = preview
            .iter()
            .find(|e| e.path == path)
            .map(|e| e.change.as_str());
        assert_eq!(
            expected,
            actual.map(|s| match s {
                git2::Delta::Added => "added",
                git2::Delta::Deleted => "deleted",
                _ => "modified",
            }),
            "staged_addition={staged_addition}"
        );
    }
}
