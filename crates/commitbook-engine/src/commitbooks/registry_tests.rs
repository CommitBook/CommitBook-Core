use super::*;
use crate::config::{CommitBookSettings, LocalConfig};
use std::fs;

fn write_commitbook(root: &std::path::Path, owner: &str, repo: &str, branch: &str) {
    let slug = slug_for(owner, repo);
    let clone = root.join(&slug);
    fs::create_dir_all(clone.join(".CommitBook").join("local")).unwrap();

    let mut config = LocalConfig::new("0 * * * *");
    config.git.branch = branch.to_string();
    config.commitbook = Some(CommitBookSettings {
        name: format!("{repo} display"),
        owner: owner.to_string(),
        repo: repo.to_string(),
        provider: "github".to_string(),
        mode: "pat".to_string(),
    });
    config.save(&clone).unwrap();
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

#[test]
fn scan_finds_one_commitbook() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");
    let cbs = scan_workspaces_root(tmp.path()).unwrap();
    assert_eq!(cbs.len(), 1);
    let cb = &cbs[0];
    assert_eq!(cb.id, "manuel/notes");
    assert_eq!(cb.owner, "manuel");
    assert_eq!(cb.repo, "notes");
    assert_eq!(cb.name, "notes display");
    assert_eq!(cb.branch, "main");
    assert_eq!(cb.auto_sync, true);
    assert!(cb.local_path.ends_with("manuel__notes"));
}

#[test]
fn scan_returns_sorted_results() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "bob", "diary", "main");
    write_commitbook(tmp.path(), "alice", "notes", "main");
    write_commitbook(tmp.path(), "alice", "todo", "main");
    let cbs = scan_workspaces_root(tmp.path()).unwrap();
    let ids: Vec<_> = cbs.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["alice/notes", "alice/todo", "bob/diary"]);
}

#[test]
fn scan_skips_config_without_commitbook_section() {
    let tmp = tempfile::tempdir().unwrap();
    let clone = tmp.path().join("legacy");
    fs::create_dir_all(clone.join(".CommitBook").join("local")).unwrap();
    let config = LocalConfig::new("0 * * * *");
    config.save(&clone).unwrap();

    let cbs = scan_workspaces_root(tmp.path()).unwrap();
    assert!(cbs.is_empty());
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
    let cb = registry::find_by_id(tmp.path(), "manuel/notes")
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
    assert_eq!(prefs.auto_sync, true);
    prefs.auto_sync = false;
    save_preferences(&clone, &prefs).unwrap();
    let loaded = load_preferences(&clone).unwrap();
    assert_eq!(loaded.auto_sync, false);
}

#[test]
fn auto_sync_reflects_per_clone_preferences() {
    let tmp = tempfile::tempdir().unwrap();
    write_commitbook(tmp.path(), "manuel", "notes", "main");

    // Default scan: auto_sync = true
    let cb = scan_workspaces_root(tmp.path()).unwrap().pop().unwrap();
    assert_eq!(cb.auto_sync, true);

    // Override via preferences
    let prefs = Preferences { auto_sync: false };
    save_preferences(&cb.local_path, &prefs).unwrap();

    let cb = scan_workspaces_root(tmp.path()).unwrap().pop().unwrap();
    assert_eq!(cb.auto_sync, false);
}

#[test]
fn init_writes_config_with_commitbook_section() {
    let tmp = tempfile::tempdir().unwrap();
    init::init_dot_commitbook(
        tmp.path(),
        "Personal Notes",
        "manuel",
        "notes",
        "main",
        "github",
        "pat",
    )
    .unwrap();

    let config = LocalConfig::load(tmp.path()).unwrap();
    let cb = config.commitbook.expect("expected [commitbook] section");
    assert_eq!(cb.name, "Personal Notes");
    assert_eq!(cb.owner, "manuel");
    assert_eq!(cb.repo, "notes");
    assert_eq!(cb.provider, "github");
    assert_eq!(cb.mode, "pat");
    assert_eq!(config.git.branch, "main");

    // .gitignore created with the local entry.
    let gi = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(gi.contains(".CommitBook/local/"));
}

#[test]
fn init_is_idempotent_on_commitbook_section() {
    let tmp = tempfile::tempdir().unwrap();
    init::init_dot_commitbook(
        tmp.path(), "First", "manuel", "notes", "main", "github", "pat",
    )
    .unwrap();
    // Second call with different name doesn't overwrite.
    init::init_dot_commitbook(
        tmp.path(), "Second", "manuel", "notes", "main", "github", "pat",
    )
    .unwrap();
    let config = LocalConfig::load(tmp.path()).unwrap();
    assert_eq!(config.commitbook.unwrap().name, "First");
}
