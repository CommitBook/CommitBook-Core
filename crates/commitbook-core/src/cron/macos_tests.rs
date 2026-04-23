use super::*;

#[test]
fn test_plist_label_prefix() {
    let label = plist_label(Path::new("/tmp/my-repo"));
    assert!(label.starts_with("com.zaai.commitbook."));
}

#[test]
fn test_plist_label_deterministic() {
    let a = plist_label(Path::new("/tmp/my-repo"));
    let b = plist_label(Path::new("/tmp/my-repo"));
    assert_eq!(a, b);
}

#[test]
fn test_plist_label_unique_per_path() {
    let a = plist_label(Path::new("/tmp/repo-a"));
    let b = plist_label(Path::new("/tmp/repo-b"));
    assert_ne!(a, b);
}

#[test]
fn test_plist_path_format() {
    let path = plist_path(Path::new("/tmp/my-repo"));
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("LaunchAgents"));
    assert!(path_str.ends_with(".plist"));
}

#[test]
fn test_xml_escape_special_chars() {
    assert_eq!(xml_escape("a&b"), "a&amp;b");
    assert_eq!(xml_escape("a<b>c"), "a&lt;b&gt;c");
    assert_eq!(xml_escape(r#"a"b"#), "a&quot;b");
    assert_eq!(xml_escape("a'b"), "a&apos;b");
    assert_eq!(xml_escape("/normal/path"), "/normal/path");
}

#[test]
fn test_generate_plist_escapes_paths() {
    let plist = generate_plist(
        Path::new("/tmp/my&repo"),
        "*/5 * * * *",
        Path::new("/usr/bin/commit<book"),
    );
    assert!(plist.contains("my&amp;repo"));
    assert!(plist.contains("commit&lt;book"));
    assert!(!plist.contains("my&repo"));
}

#[test]
fn test_generate_plist_uses_run_subcommand() {
    let plist = generate_plist(
        Path::new("/tmp/repo"),
        "0 * * * *",
        Path::new("/usr/bin/commitbook"),
    );
    assert!(plist.contains("<string>run</string>"));
    assert!(!plist.contains("auto-commit"));
    assert!(!plist.contains("--repo"));
}

#[test]
fn test_generate_plist_has_working_directory() {
    let plist = generate_plist(
        Path::new("/tmp/my&repo"),
        "0 * * * *",
        Path::new("/usr/bin/commitbook"),
    );
    assert!(plist.contains("<key>WorkingDirectory</key>"));
    // The repo path should appear escaped as WorkingDirectory value
    assert!(plist.contains("my&amp;repo"));
}

#[test]
fn test_filter_protected_strips_downloads_desktop_documents_icloud() {
    let home = Path::new("/Users/test");
    let entries = vec![
        "/Users/test/Downloads/Chrome/google-cloud-sdk/bin".to_string(),
        "/Users/test/Desktop/tools/bin".to_string(),
        "/Users/test/Documents/scripts".to_string(),
        "/Users/test/Library/Mobile Documents/com~apple~CloudDocs/bin".to_string(),
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/Users/test/.cargo/bin".to_string(),
    ];
    let filtered = filter_protected(entries, home);
    assert_eq!(
        filtered,
        vec![
            "/opt/homebrew/bin".to_string(),
            "/usr/local/bin".to_string(),
            "/Users/test/.cargo/bin".to_string(),
        ]
    );
}

#[test]
fn test_filter_protected_preserves_unrelated_entries() {
    let home = Path::new("/Users/alice");
    let entries = vec![
        "/usr/bin".to_string(),
        "/opt/homebrew/bin".to_string(),
        "/Users/alice/.local/bin".to_string(),
    ];
    let filtered = filter_protected(entries.clone(), home);
    assert_eq!(filtered, entries);
}

#[test]
fn test_filter_protected_matches_only_as_directory_prefix() {
    // "DownloadsBackup" should NOT be stripped — only real Downloads/*.
    let home = Path::new("/Users/bob");
    let entries = vec![
        "/Users/bob/DownloadsBackup/bin".to_string(),
        "/Users/bob/Downloads/bin".to_string(),
    ];
    let filtered = filter_protected(entries, home);
    assert_eq!(filtered, vec!["/Users/bob/DownloadsBackup/bin".to_string()]);
}

#[test]
fn test_build_plist_path_excludes_protected_roots() {
    let path = build_plist_path();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy().to_string();
        for sub in &["Downloads", "Desktop", "Documents", "Library/Mobile Documents"] {
            let protected_root = format!("{}/{}", home_str, sub);
            for entry in path.split(':') {
                assert!(
                    !entry.starts_with(&protected_root),
                    "entry {entry} leaks into protected root {protected_root}"
                );
            }
        }
    }
}

#[test]
fn test_build_plist_path_includes_baseline() {
    let path = build_plist_path();
    let entries: Vec<&str> = path.split(':').collect();
    assert!(entries.contains(&"/usr/bin"));
    assert!(entries.contains(&"/bin"));
}

#[test]
fn test_build_plist_path_has_no_duplicate_entries() {
    let path = build_plist_path();
    let entries: Vec<&str> = path.split(':').collect();
    let mut seen = std::collections::HashSet::new();
    for e in &entries {
        assert!(seen.insert(*e), "duplicate entry: {e}");
    }
}

#[test]
fn test_generate_plist_path_excludes_protected_roots() {
    let plist = generate_plist(
        Path::new("/tmp/repo"),
        "0 * * * *",
        Path::new("/usr/bin/commitbook"),
    );
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy().to_string();
        for sub in &["Downloads", "Desktop", "Documents"] {
            let protected_root = format!("{}/{}", home_str, sub);
            assert!(
                !plist.contains(&protected_root),
                "plist PATH leaks protected root: {protected_root}"
            );
        }
    }
}
