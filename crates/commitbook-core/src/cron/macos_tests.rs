use super::*;

#[test]
fn test_plist_label_prefix() {
    let label = plist_label(Path::new("/tmp/my-repo"));
    assert!(label.starts_with("com.commitbook."));
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
