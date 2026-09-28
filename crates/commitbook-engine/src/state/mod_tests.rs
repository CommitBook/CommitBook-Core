use super::*;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_find_commitbook_dir_finds_in_git_repo() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join(".CommitBook")).unwrap();

    let found = find_commitbook_dir_from(repo).unwrap();
    assert_eq!(found, repo.join(".CommitBook"));
}

#[test]
fn test_find_commitbook_dir_finds_from_subdir() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let subdir = repo.join("notes").join("sub");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join(".CommitBook")).unwrap();

    let found = find_commitbook_dir_from(&subdir).unwrap();
    assert_eq!(found, repo.join(".CommitBook"));
}

#[test]
fn test_find_commitbook_dir_skips_commitbook_without_git() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".CommitBook")).unwrap();
    // No .git, must not return this directory

    let err = find_commitbook_dir_from(repo).unwrap_err().to_string();
    assert!(err.contains("not initialized"));
}

#[test]
fn test_find_commitbook_dir_errors_when_not_found() {
    let tmp = tempdir().unwrap();
    let err = find_commitbook_dir_from(tmp.path())
        .unwrap_err()
        .to_string();
    assert!(err.contains("not initialized"));
}

#[test]
fn test_repo_root_from_commitbook_dir() {
    let cb_dir = PathBuf::from("/home/user/notes/.CommitBook");
    assert_eq!(repo_root(&cb_dir), PathBuf::from("/home/user/notes"));
}

#[test]
fn test_initialize_creates_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    // Create a fake .git so initialize finds a git repo.
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo, "origin", "main").unwrap();

    assert!(repo.join(".CommitBook").is_dir());
    assert!(repo.join(".CommitBook/local").is_dir());
    assert!(repo.join(".CommitBook/local/logs").is_dir());
    assert!(repo.join(".CommitBook/config.toml").exists());
}

#[test]
fn test_initialize_updates_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo, "origin", "main").unwrap();

    let gitignore = std::fs::read_to_string(repo.join(".CommitBook/.gitignore")).unwrap();
    assert_eq!(gitignore, "/local/\n");
    assert!(!repo.join(".gitignore").exists());
}

#[test]
fn test_initialize_persists_remote_name() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo, "upstream", "main").unwrap();

    let config = crate::config::LocalConfig::load(repo).unwrap();
    assert_eq!(config.git.remote, "upstream");
}

#[test]
fn test_initialize_does_not_overwrite_existing_config() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    // Write a config manually.
    let cb_dir = repo.join(".CommitBook");
    std::fs::create_dir_all(&cb_dir).unwrap();
    std::fs::write(
        cb_dir.join("config.toml"),
        "config_version = \"1\"\nenabled = true\nschedule = \"custom\"\ncreated_at = \"2026-01-01T00:00:00Z\"\n",
    )
    .unwrap();

    initialize(repo, "origin", "main").unwrap();

    // Config should not be overwritten.
    let content = std::fs::read_to_string(cb_dir.join("config.toml")).unwrap();
    assert!(content.contains("custom"));
}

#[test]
fn test_initialize_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    initialize(repo, "origin", "main").unwrap();
    initialize(repo, "origin", "main").unwrap(); // Should not error.

    assert!(repo.join(".CommitBook/config.toml").exists());
}

#[test]
fn test_gitignore_does_not_duplicate_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    std::fs::create_dir_all(repo.join(".CommitBook")).unwrap();
    crate::config::LocalConfig::ensure_gitignore(repo).unwrap();
    crate::config::LocalConfig::ensure_gitignore(repo).unwrap();

    let content = std::fs::read_to_string(repo.join(".CommitBook/.gitignore")).unwrap();
    let count = content.matches("/local/").count();
    assert_eq!(count, 1);
}

#[test]
fn prepare_leaves_old_files_untouched() {
    let tmp = tempdir().unwrap();
    let cb = tmp.path().join(".CommitBook");
    std::fs::create_dir_all(cb.join("logs")).unwrap();
    for name in ["auth.toml", "state.toml", "logs/old.log", ".lock"] {
        std::fs::write(cb.join(name), "old").unwrap();
    }
    let _lock = RepoLock::acquire(tmp.path()).unwrap();
    prepare_local_state(tmp.path()).unwrap();
    for name in ["auth.toml", "state.toml", "logs/old.log", ".lock"] {
        assert_eq!(std::fs::read_to_string(cb.join(name)).unwrap(), "old");
    }
    assert!(!cb.join("local/legacy").exists());
    assert!(!cb.join("local/auth.toml").exists());
}

#[test]
fn test_ensure_initialized_does_not_auto_init() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    // A git repo without .CommitBook/ must NOT get auto-initialized.
    std::fs::create_dir_all(repo.join(".git")).unwrap();

    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(repo).unwrap();
    let result = ensure_initialized();
    std::env::set_current_dir(original).unwrap();

    assert!(result.is_err());
    assert!(!repo.join(".CommitBook").exists());
}

#[test]
fn legacy_entries_are_reported_sorted_and_left_in_place() {
    let tmp = tempdir().unwrap();
    prepare_local_state(tmp.path()).unwrap();
    assert!(legacy_metadata_entries(tmp.path()).unwrap().is_empty());
    let cb_dir = tmp.path().join(".CommitBook");
    for file in ["auth.toml", "state.toml", ".lock", "config.json"] {
        std::fs::write(cb_dir.join(file), "private fixture").unwrap();
    }
    for dir in ["base", "logs"] {
        std::fs::create_dir(cb_dir.join(dir)).unwrap();
    }
    std::fs::write(cb_dir.join("logs/launchd-stdout.log"), "old log").unwrap();

    assert_eq!(
        legacy_metadata_entries(tmp.path()).unwrap(),
        [
            ".CommitBook/.lock",
            ".CommitBook/auth.toml",
            ".CommitBook/base/",
            ".CommitBook/config.json",
            ".CommitBook/logs/",
            ".CommitBook/state.toml",
        ]
    );
    assert_eq!(
        std::fs::read_to_string(cb_dir.join("auth.toml")).unwrap(),
        "private fixture"
    );
    assert!(cb_dir.join("logs/launchd-stdout.log").exists());
    assert!(!cb_dir.join("local/auth.toml").exists());
    assert!(!cb_dir.join("local/legacy").exists());
}

#[test]
fn os_and_editor_files_are_not_legacy_entries() {
    let tmp = tempdir().unwrap();
    prepare_local_state(tmp.path()).unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    let mut names = vec![
        ".DS_Store",
        "._config.toml",
        "._.DS_Store",
        "Thumbs.db",
        "desktop.ini",
        ".config.toml.swp",
        "config.toml~",
        "#config.toml#",
        ".config.toml.abc123.tmp",
        "config 2.toml",
        ".nfs000000000001",
    ];
    if cfg!(unix) {
        names.push("Icon\r");
    }
    for name in &names {
        std::fs::write(cb_dir.join(name), "junk").unwrap();
    }

    assert!(legacy_metadata_entries(tmp.path()).unwrap().is_empty());
    for name in &names {
        assert!(cb_dir.join(name).exists(), "{name:?}");
    }
}

#[test]
fn legacy_names_match_ascii_case_insensitively() {
    let tmp = tempdir().unwrap();
    prepare_local_state(tmp.path()).unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::write(cb_dir.join("AUTH.TOML"), "private fixture").unwrap();
    std::fs::create_dir(cb_dir.join("Logs")).unwrap();

    let entries = legacy_metadata_entries(tmp.path()).unwrap();
    // A case-insensitive file system may fold `Logs` into an existing name,
    // so check membership rather than the exact list.
    assert!(
        entries.contains(&".CommitBook/AUTH.TOML".to_string()),
        "{entries:?}"
    );
    assert!(
        entries.contains(&".CommitBook/Logs/".to_string()),
        "{entries:?}"
    );
}

#[test]
fn missing_commitbook_dir_has_no_legacy_entries() {
    let tmp = tempdir().unwrap();
    assert!(legacy_metadata_entries(tmp.path()).unwrap().is_empty());
}

#[test]
fn legacy_names_exclude_the_current_layout() {
    for current in ["config.toml", ".gitignore", "devices", "local"] {
        assert!(
            !LEGACY_METADATA_ENTRIES
                .iter()
                .any(|legacy| legacy.eq_ignore_ascii_case(current)),
            "{current}"
        );
    }
}

#[test]
fn legacy_warning_lists_only_relative_names() {
    let tmp = tempdir().unwrap();
    prepare_local_state(tmp.path()).unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    std::fs::write(cb_dir.join("auth.toml"), "private fixture").unwrap();
    std::fs::create_dir(cb_dir.join("logs")).unwrap();
    std::fs::write(cb_dir.join(".DS_Store"), "finder").unwrap();

    let warning = legacy_metadata_warning(&legacy_metadata_entries(tmp.path()).unwrap());
    assert!(
        warning.contains(".CommitBook/auth.toml, .CommitBook/logs/"),
        "{warning}"
    );
    assert!(!warning.contains(".DS_Store"), "{warning}");
    assert!(
        !warning.contains(&tmp.path().display().to_string()),
        "{warning}"
    );
    assert!(!warning.contains("fresh clone"), "{warning}");
    assert!(!warning.contains('\u{2014}'), "{warning}");
}
