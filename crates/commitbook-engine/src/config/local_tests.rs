use super::*;

fn init_repo_with_remote(path: &Path, remote: &str) {
    let repo = git2::Repository::init(path).unwrap();
    repo.remote(remote, "https://example.invalid/notes.git")
        .unwrap();
}

#[test]
fn test_default_field_values() {
    let cfg = LocalConfig::new("0 * * * *");
    assert!(cfg.enabled);
    assert_eq!(cfg.schedule, "0 * * * *");
    assert!(cfg.scheduler_id.is_none());
    assert!(cfg.git.auto_push);
    assert_eq!(cfg.git.branch, "main");
    assert_eq!(cfg.logging.level, "info");
    assert_eq!(cfg.logging.max_log_days, 30);
}

#[test]
fn test_commitbook_dir_path() {
    let dir = LocalConfig::commitbook_dir(Path::new("/tmp/repo"));
    assert_eq!(dir, PathBuf::from("/tmp/repo/.CommitBook"));
}

#[test]
fn test_config_path() {
    let p = LocalConfig::config_path(Path::new("/tmp/repo"));
    assert_eq!(p, PathBuf::from("/tmp/repo/.CommitBook/config.toml"));
}

#[test]
fn test_logs_dir_path() {
    let p = LocalConfig::logs_dir(Path::new("/tmp/repo"));
    assert_eq!(p, PathBuf::from("/tmp/repo/.CommitBook/local/logs"));
}

#[test]
fn test_lock_path() {
    let p = LocalConfig::lock_path(Path::new("/tmp/repo"));
    assert_eq!(p, PathBuf::from("/tmp/repo/.CommitBook/local/.lock"));
}

#[test]
fn test_save_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    let original = LocalConfig::new("*/15 * * * *");
    original.save(repo).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert_eq!(loaded.schedule, "*/15 * * * *");
    assert!(loaded.enabled);
    assert_eq!(loaded.git.branch, "main");
}

#[test]
fn test_load_nonexistent_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let result = LocalConfig::load(tmp.path());
    assert!(result.is_err());
}

#[test]
fn test_init_creates_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();

    let cfg = LocalConfig::init(repo, "hourly").unwrap();
    assert_eq!(cfg.schedule, "hourly");

    // Directories created
    assert!(LocalConfig::commitbook_dir(repo).exists());
    assert!(LocalConfig::logs_dir(repo).exists());

    // Config file created
    assert!(LocalConfig::config_path(repo).exists());

    // Nested ignore protects device-local state without changing the root.
    let gitignore = fs::read_to_string(repo.join(".CommitBook/.gitignore")).unwrap();
    assert_eq!(gitignore, "/local/\n");
    assert!(!repo.join(".gitignore").exists());
}

#[test]
fn test_init_preserves_repository_root_gitignore() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join(".gitignore"), "user-owned/\n").unwrap();

    LocalConfig::init(tmp.path(), "hourly").unwrap();

    assert_eq!(
        fs::read_to_string(tmp.path().join(".gitignore")).unwrap(),
        "user-owned/\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join(".CommitBook/.gitignore")).unwrap(),
        "/local/\n"
    );
}

#[test]
fn test_new_has_config_version() {
    let cfg = LocalConfig::new("hourly");
    assert_eq!(cfg.config_version, "1");
}

#[test]
fn test_load_without_version_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "upstream");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    // Write TOML without config_version field
    let toml_content = r#"
enabled = true
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert_eq!(loaded.config_version, "1");
    assert_eq!(loaded.git.remote, "upstream");

    let saved = fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(saved.contains("config_version = \"1\""));
}

#[test]
fn test_read_only_load_normalizes_without_rewriting_config() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "upstream");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();
    let original = "schedule = \"hourly\"\ncreated_at = \"now\"\n";
    fs::write(dir.join("config.toml"), original).unwrap();

    let loaded = LocalConfig::load_read_only(repo).unwrap();
    assert_eq!(loaded.config_version, "1");
    assert!(loaded.enabled);
    assert_eq!(loaded.git.remote, "upstream");
    assert_eq!(
        fs::read_to_string(dir.join("config.toml")).unwrap(),
        original
    );
}

#[test]
fn test_load_legacy_version_defaults_enabled_and_rewrites() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    let toml_content = r#"
version = "1.0.0"
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert_eq!(loaded.config_version, "1");
    assert!(loaded.enabled);

    let saved = fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(saved.contains("config_version = \"1\""));
    assert!(saved.contains("enabled = true"));
    assert!(!saved.contains("version = \"1.0.0\""));
}

#[test]
fn test_load_with_config_version_defaults_enabled_and_rewrites() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    let toml_content = r#"
config_version = "1"
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert_eq!(loaded.config_version, "1");
    assert!(loaded.enabled);

    let saved = fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(saved.contains("config_version = \"1\""));
    assert!(saved.contains("enabled = true"));
}

#[test]
fn test_load_preserves_explicit_enabled_false() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    let toml_content = r#"
config_version = "1"
enabled = false
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert!(!loaded.enabled);
}

#[test]
fn test_migrate_returns_false_when_current() {
    let mut cfg = LocalConfig::new("hourly");
    assert!(!cfg.migrate());
}

#[test]
fn test_max_log_files_alias_backwards_compat() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    // Old config using the legacy field name
    let toml_content = r#"
enabled = true
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
config_version = "1"

[logging]
level = "info"
max_log_files = 14
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert_eq!(loaded.logging.max_log_days, 14);
}

#[test]
fn test_load_missing_schedule_uses_default() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    let toml_content = r#"
version = "1.0.0"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let config = LocalConfig::load(repo).unwrap();
    assert_eq!(config.schedule, "0 * * * *");
    assert!(config.enabled);
}

#[test]
fn test_load_rejects_unknown_config_version() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    let toml_content = r#"
config_version = "2.0.0"
enabled = true
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let err = LocalConfig::load(repo).unwrap_err().to_string();
    assert!(err.contains("Unsupported local config version `2.0.0`"));
}

#[test]
fn test_commit_ai_messages_defaults_false() {
    assert!(!CommitSettings::default().ai_messages);
    assert!(!LocalConfig::new("hourly").commit.ai_messages);
}

#[test]
fn test_load_without_commit_section_defaults_false() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    // Config predating the [commit] section: ai_messages must default to false.
    let toml_content = r#"
config_version = "1"
enabled = true
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert!(!loaded.commit.ai_messages);
}

#[test]
fn test_load_commit_ai_messages_false_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    init_repo_with_remote(repo, "origin");
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    let toml_content = r#"
config_version = "1"
enabled = true
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"

[commit]
ai_messages = false
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert!(!loaded.commit.ai_messages);
}

#[test]
fn test_missing_remote_rejects_ambiguous_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    repo.remote("one", "https://example.invalid/one.git")
        .unwrap();
    repo.remote("two", "https://example.invalid/two.git")
        .unwrap();
    let cb_dir = tmp.path().join(".CommitBook");
    fs::create_dir_all(&cb_dir).unwrap();
    fs::write(
        cb_dir.join("config.toml"),
        "config_version = \"1\"\nenabled = true\nschedule = \"hourly\"\ncreated_at = \"now\"\n",
    )
    .unwrap();

    let error = LocalConfig::load(tmp.path()).unwrap_err().to_string();
    assert!(error.contains("has 2 remotes"));
    assert!(error.contains("set git.remote"));
}

#[test]
fn test_removed_files_table_has_no_effect_and_is_dropped_on_save() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo_with_remote(tmp.path(), "origin");
    let cb_dir = tmp.path().join(".CommitBook");
    fs::create_dir_all(&cb_dir).unwrap();
    fs::write(
        cb_dir.join("config.toml"),
        r#"config_version = "1"
enabled = true
schedule = "hourly"
created_at = "now"

[files]
include = ["**/*.md"]
exclude = ["private/**"]
"#,
    )
    .unwrap();

    let config = LocalConfig::load(tmp.path()).unwrap();
    config.save(tmp.path()).unwrap();
    let saved = fs::read_to_string(cb_dir.join("config.toml")).unwrap();
    assert!(!saved.contains("[files]"));
    assert!(!saved.contains("include"));
    assert!(!saved.contains("exclude"));
}

#[cfg(unix)]
#[test]
fn test_load_rejects_symlinked_commitbook_directory() {
    use std::os::unix::fs::symlink;

    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(
        outside.path().join("config.toml"),
        "config_version = \"1\"\nenabled = true\nschedule = \"hourly\"\ncreated_at = \"now\"\n[git]\nremote = \"origin\"\nbranch = \"main\"\nauto_push = true\n",
    )
    .unwrap();
    symlink(outside.path(), repo.path().join(".CommitBook")).unwrap();

    assert!(LocalConfig::load(repo.path()).is_err());
    assert!(!LocalConfig::exists(repo.path()));
}

#[cfg(unix)]
#[test]
fn test_config_io_rejects_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".CommitBook")).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "outside sentinel\n").unwrap();
    symlink(outside.path(), repo.path().join(".CommitBook/config.toml")).unwrap();

    assert!(LocalConfig::load(repo.path()).is_err());
    assert!(LocalConfig::new("hourly").save(repo.path()).is_err());
    assert_eq!(
        fs::read_to_string(outside.path()).unwrap(),
        "outside sentinel\n"
    );
}

#[cfg(unix)]
#[test]
fn test_gitignore_io_rejects_symlink_without_touching_target() {
    use std::os::unix::fs::symlink;

    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".CommitBook")).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "outside sentinel\n").unwrap();
    symlink(outside.path(), repo.path().join(".CommitBook/.gitignore")).unwrap();

    assert!(LocalConfig::ensure_gitignore(repo.path()).is_err());
    assert_eq!(
        fs::read_to_string(outside.path()).unwrap(),
        "outside sentinel\n"
    );
}

#[test]
fn test_commit_setting_missing_field_and_explicit_values_round_trip() {
    for (section, expected) in [
        ("[commit]", false),
        ("[commit]\nai_messages = false", false),
        ("[commit]\nai_messages = true", true),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        init_repo_with_remote(tmp.path(), "origin");
        let config = LocalConfig::init(tmp.path(), "hourly").unwrap();
        assert!(!config.commit.ai_messages);
        let path = LocalConfig::config_path(tmp.path());
        let content = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            content.replace("[commit]\nai_messages = false", section),
        )
        .unwrap();
        let loaded = LocalConfig::load(tmp.path()).unwrap();
        assert_eq!(loaded.commit.ai_messages, expected, "{section}");
        loaded.save(tmp.path()).unwrap();
        assert_eq!(
            LocalConfig::load(tmp.path()).unwrap().commit.ai_messages,
            expected
        );
    }
}
