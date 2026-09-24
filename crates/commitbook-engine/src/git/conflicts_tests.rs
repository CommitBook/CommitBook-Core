use super::*;
use crate::git::operations::MergeOutcome;
use crate::git::test_support::{clone_second_workdir, setup_repo_with_bare_remote};

fn establish_conflict(
    path: &str,
    base: &[u8],
    local: &[u8],
    remote: &[u8],
) -> crate::git::test_support::RepoFixture {
    let fixture = setup_repo_with_bare_remote();
    if let Some(parent) = fixture.repo_dir.path().join(path).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(fixture.repo_dir.path().join(path), base).unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("add shared file").unwrap();
    fixture.repo.push("origin", &fixture.branch).unwrap();

    std::fs::write(fixture.repo_dir.path().join(path), local).unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("local edit").unwrap();

    let other = clone_second_workdir(fixture.remote_dir.path(), &fixture.branch);
    if let Some(parent) = other.path().join(path).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(other.path().join(path), remote).unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote edit").unwrap();
    other_repo.push("origin", &fixture.branch).unwrap();

    assert!(matches!(
        fixture
            .repo
            .merge_from_remote("origin", &fixture.branch)
            .unwrap(),
        MergeOutcome::Conflicts(_)
    ));
    fixture
}

#[test]
fn structured_conflict_reads_all_index_stages() {
    let fixture = establish_conflict("shared.md", b"base\n", b"local\n", b"remote\n");

    let conflicts = fixture.repo.list_conflicts_structured().unwrap();
    assert_eq!(conflicts.len(), 1);
    let conflict = &conflicts[0];
    assert_eq!(conflict.path, "shared.md");
    assert_eq!(conflict.classification(), "content");
    assert_eq!(conflict.ancestor_text(), Some("base\n"));
    assert_eq!(conflict.local_text(), Some("local\n"));
    assert_eq!(conflict.remote_text(), Some("remote\n"));
}

#[test]
fn add_add_conflict_has_no_ancestor_stage() {
    let fixture = setup_repo_with_bare_remote();
    std::fs::write(fixture.repo_dir.path().join("new.md"), "local\n").unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("local add").unwrap();

    let other = clone_second_workdir(fixture.remote_dir.path(), &fixture.branch);
    std::fs::write(other.path().join("new.md"), "remote\n").unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote add").unwrap();
    other_repo.push("origin", &fixture.branch).unwrap();

    assert!(matches!(
        fixture
            .repo
            .merge_from_remote("origin", &fixture.branch)
            .unwrap(),
        MergeOutcome::Conflicts(_)
    ));
    let conflict = fixture.repo.find_conflict("new.md").unwrap().unwrap();
    assert_eq!(conflict.classification(), "add_add");
    assert!(conflict.ancestor.is_none());
    assert_eq!(conflict.local_text(), Some("local\n"));
    assert_eq!(conflict.remote_text(), Some("remote\n"));
}

#[test]
fn text_resolution_stages_content_and_clears_index_conflict() {
    let fixture = establish_conflict("shared.md", b"base\n", b"local\n", b"remote\n");

    fixture
        .repo
        .resolve_conflict_with_text("shared.md", "resolved\n")
        .unwrap();

    assert!(fixture.repo.list_conflicts_structured().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(fixture.repo_dir.path().join("shared.md")).unwrap(),
        "resolved\n"
    );
}

#[test]
fn selecting_absent_remote_side_stages_modify_delete_as_deletion() {
    let fixture = setup_repo_with_bare_remote();
    std::fs::write(fixture.repo_dir.path().join("shared.md"), "base\n").unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("add shared").unwrap();
    fixture.repo.push("origin", &fixture.branch).unwrap();

    std::fs::write(fixture.repo_dir.path().join("shared.md"), "local\n").unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("local edit").unwrap();

    let other = clone_second_workdir(fixture.remote_dir.path(), &fixture.branch);
    std::fs::remove_file(other.path().join("shared.md")).unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote delete").unwrap();
    other_repo.push("origin", &fixture.branch).unwrap();

    assert!(matches!(
        fixture
            .repo
            .merge_from_remote("origin", &fixture.branch)
            .unwrap(),
        MergeOutcome::Conflicts(_)
    ));
    let conflict = fixture.repo.find_conflict("shared.md").unwrap().unwrap();
    assert_eq!(conflict.classification(), "modify_delete");
    assert!(conflict.remote.is_none());

    fixture
        .repo
        .resolve_conflict_with_side("shared.md", conflict.remote.as_ref())
        .unwrap();
    assert!(fixture.repo.list_conflicts_structured().unwrap().is_empty());
    assert!(!fixture.repo_dir.path().join("shared.md").exists());
}

#[test]
fn binary_conflict_is_never_eligible_for_text_resolution() {
    let fixture = establish_conflict(
        "image.md",
        b"base\0bytes",
        b"local\0bytes",
        b"remote\0bytes",
    );
    let conflict = fixture.repo.find_conflict("image.md").unwrap().unwrap();
    assert_eq!(conflict.classification(), "binary");
    assert!(conflict.is_binary_or_special());
    assert!(fixture
        .repo
        .resolve_conflict_with_text("image.md", "not binary anymore")
        .is_err());
}

#[cfg(unix)]
#[test]
fn taking_executable_side_updates_worktree_mode() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let fixture = setup_repo_with_bare_remote();
    let path = fixture.repo_dir.path().join("script.md");
    std::fs::write(&path, "base\n").unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("add script").unwrap();
    fixture.repo.push("origin", &fixture.branch).unwrap();

    let repository = git2::Repository::open(fixture.repo_dir.path()).unwrap();
    repository
        .config()
        .unwrap()
        .set_bool("core.filemode", true)
        .unwrap();
    std::fs::write(&path, "local executable\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("make script executable").unwrap();

    let other = clone_second_workdir(fixture.remote_dir.path(), &fixture.branch);
    std::fs::write(other.path().join("script.md"), "remote regular\n").unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote script edit").unwrap();
    other_repo.push("origin", &fixture.branch).unwrap();

    assert!(matches!(
        fixture
            .repo
            .merge_from_remote("origin", &fixture.branch)
            .unwrap(),
        MergeOutcome::Conflicts(_)
    ));
    let conflict = fixture.repo.find_conflict("script.md").unwrap().unwrap();
    let local = conflict.local.as_ref().unwrap();
    assert_eq!(local.mode, 0o100755);
    fixture
        .repo
        .resolve_conflict_with_side("script.md", Some(local))
        .unwrap();

    let repository = git2::Repository::open(fixture.repo_dir.path()).unwrap();
    let entry = repository
        .index()
        .unwrap()
        .get_path(Path::new("script.md"), 0)
        .unwrap();
    assert_eq!(entry.mode, 0o100755);
    assert_ne!(std::fs::metadata(&path).unwrap().mode() & 0o111, 0);
    let status = repository.status_file(Path::new("script.md")).unwrap();
    assert!(!status.intersects(
        git2::Status::WT_MODIFIED
            | git2::Status::WT_TYPECHANGE
            | git2::Status::WT_DELETED
            | git2::Status::WT_NEW
    ));
}

fn stage_gitlink(repo_path: &Path, path: &str, oid: git2::Oid) {
    let repository = git2::Repository::open(repo_path).unwrap();
    let mut index = repository.index().unwrap();
    let _ = index.remove_path(Path::new(path));
    index
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: 0o160000,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: oid,
            flags: 0,
            flags_extended: 0,
            path: path.as_bytes().to_vec(),
        })
        .unwrap();
    index.write().unwrap();
}

#[test]
fn selecting_absent_gitlink_side_keeps_submodule_worktree_directory() {
    let fixture = setup_repo_with_bare_remote();
    let submodule_worktree = fixture.repo_dir.path().join("modules/book");
    std::fs::create_dir_all(&submodule_worktree).unwrap();
    let submodule_repository = git2::Repository::init(&submodule_worktree).unwrap();
    let mut config = submodule_repository.config().unwrap();
    config.set_str("user.name", "Test").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
    drop(config);
    std::fs::write(submodule_worktree.join("sentinel"), "base\n").unwrap();
    let submodule = GitRepo::open(&submodule_worktree).unwrap();
    submodule.stage_all().unwrap();
    submodule.commit("base submodule").unwrap();
    let initial_target = git2::Oid::from_str(&submodule.rev_parse("HEAD").unwrap()).unwrap();
    stage_gitlink(fixture.repo_dir.path(), "modules/book", initial_target);
    GitRepo::open(fixture.repo_dir.path())
        .unwrap()
        .commit("add gitlink")
        .unwrap();
    fixture.repo.push("origin", &fixture.branch).unwrap();

    let other = clone_second_workdir(fixture.remote_dir.path(), &fixture.branch);
    std::fs::write(submodule_worktree.join("sentinel"), "keep me\n").unwrap();
    submodule.stage_all().unwrap();
    submodule.commit("update submodule").unwrap();
    let local_target = git2::Oid::from_str(&submodule.rev_parse("HEAD").unwrap()).unwrap();
    stage_gitlink(fixture.repo_dir.path(), "modules/book", local_target);
    GitRepo::open(fixture.repo_dir.path())
        .unwrap()
        .commit("update gitlink locally")
        .unwrap();

    let other_repository = git2::Repository::open(other.path()).unwrap();
    let mut other_index = other_repository.index().unwrap();
    other_index.remove_path(Path::new("modules/book")).unwrap();
    other_index.write().unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.commit("delete gitlink remotely").unwrap();
    other_repo.push("origin", &fixture.branch).unwrap();

    assert!(matches!(
        fixture
            .repo
            .merge_from_remote("origin", &fixture.branch)
            .unwrap(),
        MergeOutcome::Conflicts(_)
    ));
    let conflict = fixture.repo.find_conflict("modules/book").unwrap().unwrap();
    assert!(conflict.local.as_ref().unwrap().is_gitlink());
    assert!(conflict.remote.is_none());
    // Gitlinks report their own kind so callers can render submodule-specific
    // messaging, while still being excluded from text resolution.
    assert_eq!(conflict.classification(), "gitlink");
    assert!(conflict.is_binary_or_special());

    fixture
        .repo
        .resolve_conflict_with_side("modules/book", conflict.remote.as_ref())
        .unwrap();
    assert!(fixture.repo.list_conflicts_structured().unwrap().is_empty());
    assert!(submodule_worktree.is_dir());
    assert_eq!(
        std::fs::read_to_string(submodule_worktree.join("sentinel")).unwrap(),
        "keep me\n"
    );
    assert!(git2::Repository::open(fixture.repo_dir.path())
        .unwrap()
        .index()
        .unwrap()
        .get_path(Path::new("modules/book"), 0)
        .is_none());
}

#[cfg(unix)]
#[test]
fn taking_symlink_side_preserves_index_mode_without_dereferencing_target() {
    use std::os::unix::fs::symlink;

    let fixture = setup_repo_with_bare_remote();
    let local_target = tempfile::tempdir().unwrap();
    let remote_target = tempfile::tempdir().unwrap();
    std::fs::write(local_target.path().join("sentinel"), "local untouched\n").unwrap();
    std::fs::write(remote_target.path().join("sentinel"), "remote untouched\n").unwrap();

    symlink(
        local_target.path(),
        fixture.repo_dir.path().join("linked.md"),
    )
    .unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("local symlink").unwrap();

    let other = clone_second_workdir(fixture.remote_dir.path(), &fixture.branch);
    symlink(remote_target.path(), other.path().join("linked.md")).unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote symlink").unwrap();
    other_repo.push("origin", &fixture.branch).unwrap();

    assert!(matches!(
        fixture
            .repo
            .merge_from_remote("origin", &fixture.branch)
            .unwrap(),
        MergeOutcome::Conflicts(_)
    ));
    let conflict = fixture.repo.find_conflict("linked.md").unwrap().unwrap();
    assert_eq!(conflict.classification(), "symlink");
    let remote = conflict.remote.as_ref().unwrap();
    assert_eq!(remote.mode, 0o120000);
    fixture
        .repo
        .resolve_conflict_with_side("linked.md", Some(remote))
        .unwrap();

    assert_eq!(
        std::fs::read_link(fixture.repo_dir.path().join("linked.md")).unwrap(),
        remote_target.path()
    );
    assert_eq!(
        std::fs::read_to_string(local_target.path().join("sentinel")).unwrap(),
        "local untouched\n"
    );
    assert_eq!(
        std::fs::read_to_string(remote_target.path().join("sentinel")).unwrap(),
        "remote untouched\n"
    );
    let repository = git2::Repository::open(fixture.repo_dir.path()).unwrap();
    let entry = repository
        .index()
        .unwrap()
        .get_path(Path::new("linked.md"), 0)
        .unwrap();
    assert_eq!(entry.mode, 0o120000);
    assert_eq!(
        repository.find_blob(entry.id).unwrap().content(),
        remote_target.path().as_os_str().as_encoded_bytes()
    );
}

#[cfg(unix)]
#[test]
fn taking_regular_side_replaces_final_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    let fixture = establish_conflict("shared.md", b"base\n", b"local\n", b"remote\n");
    let conflict = fixture.repo.find_conflict("shared.md").unwrap().unwrap();
    let remote = conflict.remote.as_ref().unwrap().clone();
    let outside = tempfile::tempdir().unwrap();
    let outside_target = outside.path().join("target.md");
    std::fs::write(&outside_target, "outside untouched\n").unwrap();
    let worktree_path = fixture.repo_dir.path().join("shared.md");
    std::fs::remove_file(&worktree_path).unwrap();
    symlink(&outside_target, &worktree_path).unwrap();

    fixture
        .repo
        .resolve_conflict_with_side("shared.md", Some(&remote))
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(&outside_target).unwrap(),
        "outside untouched\n"
    );
    let metadata = std::fs::symlink_metadata(&worktree_path).unwrap();
    assert!(metadata.is_file());
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(std::fs::read(&worktree_path).unwrap(), remote.content);
    assert!(fixture.repo.list_conflicts_structured().unwrap().is_empty());
    let repository = git2::Repository::open(fixture.repo_dir.path()).unwrap();
    let entry = repository
        .index()
        .unwrap()
        .get_path(Path::new("shared.md"), 0)
        .unwrap();
    assert_eq!(entry.id, remote.oid);
    assert_eq!(entry.mode, remote.mode);
}

#[cfg(unix)]
#[test]
fn text_resolution_rejects_final_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    let fixture = establish_conflict("shared.md", b"base\n", b"local\n", b"remote\n");
    let outside = tempfile::tempdir().unwrap();
    let outside_target = outside.path().join("target.md");
    std::fs::write(&outside_target, "outside untouched\n").unwrap();
    let worktree_path = fixture.repo_dir.path().join("shared.md");
    std::fs::remove_file(&worktree_path).unwrap();
    symlink(&outside_target, &worktree_path).unwrap();

    let error = fixture
        .repo
        .resolve_conflict_with_text("shared.md", "resolved\n")
        .unwrap_err();

    assert!(error.to_string().contains("non-regular path"));
    assert_eq!(
        std::fs::read_to_string(&outside_target).unwrap(),
        "outside untouched\n"
    );
    assert!(std::fs::symlink_metadata(&worktree_path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fixture.repo.list_conflicts_structured().unwrap().len(), 1);
}

#[cfg(unix)]
#[test]
fn text_resolution_rejects_symlinked_parent() {
    use std::os::unix::fs::symlink;

    let fixture = establish_conflict("notes/shared.md", b"base\n", b"local\n", b"remote\n");
    let outside = tempfile::tempdir().unwrap();
    std::fs::remove_file(fixture.repo_dir.path().join("notes/shared.md")).unwrap();
    std::fs::remove_dir(fixture.repo_dir.path().join("notes")).unwrap();
    symlink(outside.path(), fixture.repo_dir.path().join("notes")).unwrap();

    let result = fixture
        .repo
        .resolve_conflict_with_text("notes/shared.md", "resolved\n");
    assert!(result.is_err());
    assert!(!outside.path().join("shared.md").exists());
}

#[test]
fn marker_validation_rejects_all_marker_sections() {
    assert!(has_conflict_markers("<<<<<<< ours\n"));
    assert!(has_conflict_markers("||||||| base\n"));
    assert!(!has_conflict_markers("=======\n"));
    assert!(has_conflict_markers(">>>>>>> theirs\n"));
    assert!(has_conflict_markers("  <<<<<<< ours\n"));
    assert!(has_conflict_markers("\t||||||| base\n"));
    assert!(has_conflict_markers("    >>>>>>> theirs\n"));
    assert!(!has_conflict_markers("ordinary ======= prose\n"));
}

const ABOUT_BASE: &str = "# 20260905\n\n## Request 1\n\n- Als Dreijähriger mit Auto fahren\n- 2 Töchter\n- Math MG.com Hero image\n\n";
const ABOUT_LOCAL: &str = "# 20260905\n\n## Request 1\n\n- Als Dreijähriger mit Auto fahren\n- 2 Töchter\n- Math MG.com Hero image\n- Builder Bucket List\n  - 3d model of my home\n  - Car with light layout\n  - Song for Apres Ski\n\n\n\n\n\n";
const ABOUT_REMOTE: &str = "# 20260905\n\n## Request 1\n\n- Als Dreijähriger mit Auto fahren\n- 2 Töchter\n- Math MG.com Hero image\n- In Liechtenstein I was skiing faster than driving a car\n";

#[test]
fn keep_both_keeps_both_additions_local_first() {
    let fixture = establish_conflict(
        "About.md",
        ABOUT_BASE.as_bytes(),
        ABOUT_LOCAL.as_bytes(),
        ABOUT_REMOTE.as_bytes(),
    );

    assert!(fixture.repo.try_resolve_both("About.md").unwrap());

    assert!(fixture.repo.list_conflicts_structured().unwrap().is_empty());
    let merged = std::fs::read_to_string(fixture.repo_dir.path().join("About.md")).unwrap();
    assert!(!has_conflict_markers(&merged));
    assert!(merged.starts_with("# 20260905\n\n## Request 1\n\n- Als Dreijähriger"));
    assert_eq!(merged.matches("# 20260905").count(), 1);
    let bucket = merged.find("- Builder Bucket List").unwrap();
    let skiing = merged.find("- In Liechtenstein").unwrap();
    assert!(bucket < skiing, "local additions come first:\n{merged}");
    assert!(merged.contains("  - Song for Apres Ski\n"));
}

#[test]
fn keep_both_keeps_both_versions_of_an_edited_line() {
    let fixture = establish_conflict(
        "shared.md",
        b"intro\nMeeting at 3pm\noutro\n",
        b"intro\nMeeting at 4pm\noutro\n",
        b"intro\nMeeting at 5pm\noutro\n",
    );

    assert!(fixture.repo.try_resolve_both("shared.md").unwrap());

    assert!(fixture.repo.list_conflicted_paths().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(fixture.repo_dir.path().join("shared.md")).unwrap(),
        "intro\nMeeting at 4pm\nMeeting at 5pm\noutro\n"
    );
}

#[test]
fn keep_both_handles_an_edit_against_an_append() {
    // Remote rewrites the last line while local appends after it.
    let fixture = establish_conflict(
        "shared.md",
        b"one\ntwo\n",
        b"one\ntwo\nthree\n",
        b"one\nTWO\n",
    );

    assert!(fixture.repo.try_resolve_both("shared.md").unwrap());
    let merged = std::fs::read_to_string(fixture.repo_dir.path().join("shared.md")).unwrap();
    assert!(!has_conflict_markers(&merged));
    assert!(
        merged.contains("three") && merged.contains("TWO"),
        "{merged}"
    );
}

#[test]
fn keep_both_joins_an_add_add_conflict() {
    let fixture = setup_repo_with_bare_remote();
    std::fs::write(fixture.repo_dir.path().join("new.md"), "local\n").unwrap();
    fixture.repo.stage_all().unwrap();
    fixture.repo.commit("local add").unwrap();
    let other = clone_second_workdir(fixture.remote_dir.path(), &fixture.branch);
    std::fs::write(other.path().join("new.md"), "remote\n").unwrap();
    let other_repo = GitRepo::open(other.path()).unwrap();
    other_repo.stage_all().unwrap();
    other_repo.commit("remote add").unwrap();
    other_repo.push("origin", &fixture.branch).unwrap();
    fixture
        .repo
        .merge_from_remote("origin", &fixture.branch)
        .unwrap();

    assert!(fixture.repo.try_resolve_both("new.md").unwrap());
    assert_eq!(
        std::fs::read_to_string(fixture.repo_dir.path().join("new.md")).unwrap(),
        "local\nremote\n"
    );
}

#[test]
fn keep_both_leaves_binary_conflicts() {
    let fixture = establish_conflict("blob.bin", b"\0base", b"\0base\0local", b"\0base\0remote");

    assert!(!fixture.repo.try_resolve_both("blob.bin").unwrap());
    assert_eq!(
        fixture.repo.list_conflicted_paths().unwrap(),
        vec!["blob.bin"]
    );
}

#[test]
fn keep_both_leaves_text_that_looks_like_markers() {
    let fixture = establish_conflict(
        "shared.md",
        b"intro\nbase\n",
        b"intro\n<<<<<<< quoted marker\n",
        b"intro\nremote\n",
    );

    assert!(!fixture.repo.try_resolve_both("shared.md").unwrap());
    assert_eq!(
        fixture.repo.list_conflicted_paths().unwrap(),
        vec!["shared.md"]
    );
}
