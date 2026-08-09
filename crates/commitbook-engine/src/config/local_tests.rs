use super::*;

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

    // .gitignore updated
    let gitignore = fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gitignore.contains(".CommitBook/local/"));
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

    let saved = fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(saved.contains("config_version = \"1\""));
}

#[test]
fn test_load_legacy_version_defaults_enabled_and_rewrites() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
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
fn test_commit_ai_messages_defaults_true() {
    assert!(CommitSettings::default().ai_messages);
    assert!(LocalConfig::new("hourly").commit.ai_messages);
}

#[test]
fn test_load_without_commit_section_defaults_true() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let dir = repo.join(".CommitBook");
    fs::create_dir_all(&dir).unwrap();

    // Config predating the [commit] section: ai_messages must default to true.
    let toml_content = r#"
config_version = "1"
enabled = true
schedule = "0 * * * *"
created_at = "2026-04-07T00:00:00Z"
"#;
    fs::write(dir.join("config.toml"), toml_content).unwrap();

    let loaded = LocalConfig::load(repo).unwrap();
    assert!(loaded.commit.ai_messages);
}

#[test]
fn test_load_commit_ai_messages_false_parses() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
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
