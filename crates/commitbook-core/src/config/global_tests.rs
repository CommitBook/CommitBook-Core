use super::*;

#[test]
fn test_default_version() {
    let cfg = GlobalConfig::default();
    assert_eq!(cfg.version, "1.0.0");
}

#[test]
fn test_default_repos_empty() {
    let cfg = GlobalConfig::default();
    assert!(cfg.repos.is_empty());
}

#[test]
fn test_default_providers_order() {
    let cfg = GlobalConfig::default();
    assert_eq!(cfg.ai.providers.len(), 4);
    assert_eq!(cfg.ai.providers[0], "gh-copilot");
    assert_eq!(cfg.ai.providers[1], "claude-cli");
    assert_eq!(cfg.ai.providers[2], "codex-cli");
    assert_eq!(cfg.ai.providers[3], "fallback");
}

#[test]
fn test_default_no_copilot_path() {
    let cfg = GlobalConfig::default();
    assert!(cfg.ai.gh_copilot_path.is_none());
}

#[test]
fn test_toml_roundtrip() {
    let original = GlobalConfig::default();
    let serialized = toml::to_string_pretty(&original).unwrap();
    let deserialized: GlobalConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(deserialized.version, original.version);
    assert_eq!(deserialized.ai.providers, original.ai.providers);
    assert!(deserialized.repos.is_empty());
}

#[test]
fn test_migrate_returns_false_current() {
    let mut cfg = GlobalConfig::default();
    assert!(!cfg.migrate());
}

#[test]
fn test_toml_without_version_field() {
    // GlobalConfig requires version field, but repos/ai have defaults
    let toml_content = r#"version = "1.0.0""#;
    let cfg: GlobalConfig = toml::from_str(toml_content).unwrap();
    assert_eq!(cfg.version, "1.0.0");
    assert!(cfg.repos.is_empty());
    assert_eq!(cfg.ai.providers.len(), 4);
}
