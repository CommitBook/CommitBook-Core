use super::*;
use crate::config::{Auth, LocalConfig};
use std::fs;

/// A managed clone whose remote URL identifies `owner/repo`.
fn write_commitbook(root: &std::path::Path, owner: &str, repo: &str, branch: &str) {
    let slug = slug_for(owner, repo);
    let clone = root.join(&slug);
    fs::create_dir_all(&clone).unwrap();
    git2::Repository::init(&clone)
        .unwrap()
        .remote("origin", &format!("https://github.com/{owner}/{repo}.git"))
        .unwrap();
    fs::create_dir_all(clone.join(".CommitBook").join("local")).unwrap();
    LocalConfig::new(&format!("{repo} display"), branch, "origin")
        .save(&clone)
        .unwrap();
    identity::ensure(&clone).unwrap();
}

#[test]
fn scan_empty_root_returns_empty_vec() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("missing");
    let cbs = scan_workspaces_root(&root).unwrap();
    assert!(cbs.is_empty());
}

#[test]
fn scan_skips_subdirs_without_dot_commitbook() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("not-a-cb")).unwrap();
    let cbs = scan_workspaces_root(tmp.path()).unwrap();
    assert!(cbs.is_empty());
}

#[cfg(unix)]
#[test]
fn scan_never_follows_workspace_or_commitbook_symlinks() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write_commitbook(outside.path(), "outside", "notes", "main");
    symlink(
        outside.path().join("outside__notes"),
        root.path().join("linked-clone"),
    )
    .unwrap();

    let clone = root.path().join("linked-config");
    fs::create_dir(&clone).unwrap();
    symlink(
        outside.path().join("outside__notes/.CommitBook"),
        clone.join(".CommitBook"),
    )
    .unwrap();

    assert!(scan_workspaces_root(root.path()).unwrap().is_empty());
}

#[test]
fn scan_finds_one_commitbook() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");
    let cbs = scan_workspaces_root(tmp.path()).unwrap();
    assert_eq!(cbs.len(), 1);
    let cb = &cbs[0];
    assert_eq!(
        cb.commitbook_id,
        identity::load(&tmp.path().join("manuel__notes")).unwrap()
    );
    assert_eq!(cb.owner, "manuel");
    assert_eq!(cb.repo, "notes");
    assert_eq!(cb.name, "notes display");
    assert_eq!(cb.branch, "main");
    assert_eq!(cb.provider, "github");
    assert_eq!(cb.mode, "existing_local_repo");
    assert!(cb.auto_sync);
    assert!(cb.local_path.ends_with("manuel__notes"));
}

#[test]
fn scan_returns_sorted_results() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "bob", "diary", "main");
    write_commitbook(tmp.path(), "alice", "notes", "main");
    write_commitbook(tmp.path(), "alice", "todo", "main");
    let cbs = scan_workspaces_root(tmp.path()).unwrap();
    let ids: Vec<_> = cbs.iter().map(|c| c.commitbook_id.as_str()).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn scan_supports_local_remotes_without_owner() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("local-only");
    git2::Repository::init(&clone)
        .unwrap()
        .remote("origin", "notes.git")
        .unwrap();
    LocalConfig::new("Notes", "main", "origin")
        .save(&clone)
        .unwrap();
    identity::ensure(&clone).unwrap();

    let cbs = scan_workspaces_root(tmp.path()).unwrap();
    assert_eq!(cbs.len(), 1, "{cbs:?}");
}

/// A clone whose config uses the pre-0.9 format, which no longer loads.
fn write_old_format_clone(root: &std::path::Path, dir: &str) -> std::path::PathBuf {
    let clone = root.join(dir);
    git2::Repository::init(&clone).unwrap();
    fs::create_dir_all(clone.join(".CommitBook")).unwrap();
    fs::write(
        clone.join(".CommitBook/config.toml"),
        "config_version = \"1\"\nschedule = \"hourly\"\n",
    )
    .unwrap();
    clone
}

#[test]
fn scan_reports_a_broken_clone_without_hiding_the_others() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");
    write_old_format_clone(tmp.path(), "manuel__old");

    let scan = scan_workspaces(tmp.path()).unwrap();
    let ids: Vec<&str> = scan
        .commitbooks
        .iter()
        .map(|cb| cb.commitbook_id.as_str())
        .collect();
    assert_eq!(
        ids,
        [identity::load(&tmp.path().join("manuel__notes")).unwrap()]
    );
    assert_eq!(scan.broken.len(), 1);
    let error = &scan.broken[0].error;
    assert!(
        error.contains("Failed to load CommitBook config"),
        "{error}"
    );
    assert!(error.contains("manuel__old"), "{error}");
    assert!(error.contains("commitbook init"), "{error}");

    // The usable clone is still found by id.
    assert!(registry::find_by_id(
        tmp.path(),
        &identity::load(&tmp.path().join("manuel__notes")).unwrap()
    )
    .unwrap()
    .is_some());
}

#[test]
fn find_by_id_names_broken_clones_when_the_id_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    write_old_format_clone(tmp.path(), "manuel__old");
    let error = format!(
        "{:#}",
        registry::find_by_id(tmp.path(), "manuel/old").unwrap_err()
    );
    assert!(error.contains("1 clone(s) could not be loaded"), "{error}");
    assert!(error.contains("manuel__old"), "{error}");

    let clean = tempfile::tempdir().unwrap();
    assert!(registry::find_by_id(clean.path(), "manuel/old")
        .unwrap()
        .is_none());
}

#[test]
fn scan_reports_this_devices_auth_as_mode() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");
    let clone = tmp.path().join(slug_for("manuel", "notes"));
    crate::devices::register(&clone, Some("Phone"), Auth::GithubApp).unwrap();

    let cb = scan_workspaces_root(tmp.path()).unwrap().pop().unwrap();
    assert_eq!(cb.mode, "github_app");
}

#[test]
fn find_by_id_returns_none_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");
    let cb = registry::find_by_id(tmp.path(), "manuel/missing").unwrap();
    assert!(cb.is_none());
}

#[test]
fn find_by_id_returns_match() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");
    let cb = registry::find_by_id(
        tmp.path(),
        &identity::load(&tmp.path().join("manuel__notes")).unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(cb.repo, "notes");
}

#[test]
fn preferences_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().to_path_buf();
    fs::create_dir_all(clone.join(".CommitBook").join("local")).unwrap();

    let mut prefs = Preferences::default();
    assert!(prefs.auto_sync);
    prefs.auto_sync = false;
    save_preferences(&clone, &prefs).unwrap();
    let loaded = load_preferences(&clone).unwrap();
    assert!(!loaded.auto_sync);
}

#[test]
fn auto_sync_reflects_per_clone_preferences() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");

    // Default scan: auto_sync = true
    let cb = scan_workspaces_root(tmp.path()).unwrap().pop().unwrap();
    assert!(cb.auto_sync);

    // Override via preferences
    let prefs = Preferences { auto_sync: false };
    save_preferences(&cb.local_path, &prefs).unwrap();

    let cb = scan_workspaces_root(tmp.path()).unwrap().pop().unwrap();
    assert!(!cb.auto_sync);
}

#[test]
fn init_writes_config_and_registers_the_device() {
    let tmp = tempfile::tempdir().unwrap();
    init::init_dot_commitbook(
        tmp.path(),
        "Personal Notes",
        "main",
        "origin",
        Some("Phone"),
        Auth::Pat,
    )
    .unwrap();

    let config = LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.commitbook.name, "Personal Notes");
    assert_eq!(config.git.branch, "main");
    assert_eq!(config.git.remote, "origin");
    let (_, device) = crate::devices::this_device(tmp.path()).unwrap().unwrap();
    assert_eq!(device.name, "Phone");
    assert_eq!(device.auth, Auth::Pat);

    // Nested ignore created without modifying repository-root policy.
    let gi = std::fs::read_to_string(tmp.path().join(".CommitBook/.gitignore")).unwrap();
    assert_eq!(gi, "/local/\n");
    assert!(!tmp.path().join(".gitignore").exists());
}

#[test]
fn init_keeps_an_existing_config_and_device() {
    let tmp = tempfile::tempdir().unwrap();
    init::init_dot_commitbook(tmp.path(), "First", "main", "origin", None, Auth::Pat).unwrap();
    // A second call with different values changes nothing.
    init::init_dot_commitbook(
        tmp.path(),
        "Second",
        "other-branch",
        "upstream",
        Some("Renamed"),
        Auth::Ssh,
    )
    .unwrap();
    let config = LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.git.branch, "main");
    assert_eq!(config.commitbook.name, "First");
    let (id, device) = crate::devices::this_device(tmp.path()).unwrap().unwrap();
    assert_eq!(device.name, crate::devices::default_name(&id));
    assert_eq!(device.auth, Auth::Pat);
}

#[test]
fn scan_reports_missing_and_duplicate_identities_without_repair() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "one", "notes", "main");
    write_commitbook(tmp.path(), "two", "notes", "main");
    let one = tmp.path().join("one__notes");
    let two = tmp.path().join("two__notes");
    let id = identity::load(&one).unwrap();
    fs::copy(identity::path(&one), identity::path(&two)).unwrap();
    let scan = scan_workspaces(tmp.path()).unwrap();
    assert!(scan.commitbooks.is_empty());
    assert_eq!(scan.broken.len(), 2);
    assert!(registry::find_by_id(tmp.path(), &id)
        .unwrap_err()
        .to_string()
        .contains("Duplicate"));
    assert_eq!(identity::load(&two).unwrap(), id);
    fs::remove_file(identity::path(&two)).unwrap();
    let scan = scan_workspaces(tmp.path()).unwrap();
    assert_eq!(scan.commitbooks.len(), 1);
    assert_eq!(scan.broken.len(), 1);
    assert!(!identity::path(&two).exists());
}
