use super::*;
use crate::domain::section::FrontmatterFormat;

#[test]
fn test_yaml_frontmatter() {
    let content = "---\ntitle: Hello\nauthor: Manuel\n---\n\nSome content here.";
    let (fm, rest) = extract_frontmatter(content);
    let fm = fm.unwrap();
    assert_eq!(fm.format, FrontmatterFormat::Yaml);
    assert_eq!(fm.fields.get("title").unwrap(), "Hello");
    assert_eq!(fm.fields.get("author").unwrap(), "Manuel");
    assert!(rest.contains("Some content here."));
}

#[test]
fn test_toml_frontmatter() {
    let content = "+++\ntitle = \"Hello\"\nauthor = \"Manuel\"\n+++\n\nSome content here.";
    let (fm, rest) = extract_frontmatter(content);
    let fm = fm.unwrap();
    assert_eq!(fm.format, FrontmatterFormat::Toml);
    assert_eq!(fm.fields.get("title").unwrap(), "Hello");
    assert_eq!(fm.fields.get("author").unwrap(), "Manuel");
    assert!(rest.contains("Some content here."));
}

#[test]
fn test_no_frontmatter() {
    let content = "# Hello World\n\nSome content.";
    let (fm, rest) = extract_frontmatter(content);
    assert!(fm.is_none());
    assert_eq!(rest, content);
}

#[test]
fn test_yaml_quoted_values() {
    let content = "---\ntitle: \"Quoted Title\"\ntag: 'single quoted'\n---\n\nBody.";
    let (fm, _) = extract_frontmatter(content);
    let fm = fm.unwrap();
    assert_eq!(fm.fields.get("title").unwrap(), "Quoted Title");
    assert_eq!(fm.fields.get("tag").unwrap(), "single quoted");
}

#[test]
fn test_yaml_empty_value() {
    let content = "---\ntitle:\ndescription: something\n---\n\nBody.";
    let (fm, _) = extract_frontmatter(content);
    let fm = fm.unwrap();
    assert_eq!(fm.fields.get("title").unwrap(), "");
    assert_eq!(fm.fields.get("description").unwrap(), "something");
}

#[test]
fn test_yaml_with_comments() {
    let content = "---\n# This is a comment\ntitle: Hello\n---\n\nBody.";
    let (fm, _) = extract_frontmatter(content);
    let fm = fm.unwrap();
    assert_eq!(fm.fields.len(), 1);
    assert_eq!(fm.fields.get("title").unwrap(), "Hello");
}

#[test]
fn test_frontmatter_preserves_raw() {
    let content = "---\ntitle: Hello\nauthor: Manuel\n---\n\nBody.";
    let (fm, _) = extract_frontmatter(content);
    let fm = fm.unwrap();
    assert_eq!(fm.raw, "title: Hello\nauthor: Manuel");
}

#[test]
fn test_content_starting_with_dashes_not_frontmatter() {
    // Content that starts with --- but doesn't have a closing --- is not frontmatter.
    let content = "--- this is not frontmatter\nJust regular content.";
    let (fm, rest) = extract_frontmatter(content);
    assert!(fm.is_none());
    assert_eq!(rest, content);
}
