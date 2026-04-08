use super::*;

#[test]
fn test_parse_simple_document() {
    let content = "# Hello\n\nWorld.\n\n## Sub\n\nSub content.\n";
    let tree = parse_document(content);

    assert!(tree.frontmatter.is_none());
    assert!(tree.preamble.is_empty());
    assert_eq!(tree.sections.len(), 2);
    assert_eq!(tree.sections[0].path, "/Hello");
    assert_eq!(tree.sections[0].level, 1);
    assert!(tree.sections[0].content.contains("World."));
    assert_eq!(tree.sections[1].path, "/Hello/Sub");
    assert_eq!(tree.sections[1].level, 2);
    assert!(tree.sections[1].content.contains("Sub content."));
}

#[test]
fn test_parse_with_preamble() {
    let content = "This is preamble text.\n\n# First\n\nContent.\n";
    let tree = parse_document(content);

    assert!(tree.preamble.contains("This is preamble text."));
    assert_eq!(tree.sections.len(), 1);
    assert_eq!(tree.sections[0].path, "/First");
}

#[test]
fn test_parse_with_yaml_frontmatter() {
    let content = "---\ntitle: My Notes\n---\n\n# Notes\n\nSome notes.\n";
    let tree = parse_document(content);

    assert!(tree.frontmatter.is_some());
    let fm = tree.frontmatter.unwrap();
    assert_eq!(fm.fields.get("title").unwrap(), "My Notes");
    assert_eq!(tree.sections.len(), 1);
    assert_eq!(tree.sections[0].path, "/Notes");
}

#[test]
fn test_parse_with_toml_frontmatter() {
    let content = "+++\ntitle = \"My Notes\"\n+++\n\n# Notes\n\nSome notes.\n";
    let tree = parse_document(content);

    assert!(tree.frontmatter.is_some());
    let fm = tree.frontmatter.unwrap();
    assert_eq!(fm.fields.get("title").unwrap(), "My Notes");
}

#[test]
fn test_parse_no_headings() {
    let content = "Just some text without any headings.\n\nAnother paragraph.";
    let tree = parse_document(content);

    assert!(tree.frontmatter.is_none());
    assert!(tree.preamble.contains("Just some text"));
    assert!(tree.sections.is_empty());
}

#[test]
fn test_parse_empty_document() {
    let tree = parse_document("");
    assert!(tree.frontmatter.is_none());
    assert!(tree.sections.is_empty());
}

#[test]
fn test_parse_multiple_h1s() {
    let content = "# Product\n\nProduct notes.\n\n# Journal\n\nJournal entry.\n";
    let tree = parse_document(content);

    assert_eq!(tree.sections.len(), 2);
    assert_eq!(tree.sections[0].path, "/Product");
    assert_eq!(tree.sections[1].path, "/Journal");
}

#[test]
fn test_parse_nested_hierarchy() {
    let content = "# Product\n## Notes\n## Todo\n# Journal\n## 2026-04-06\n";
    let tree = parse_document(content);

    assert_eq!(tree.sections.len(), 5);
    assert_eq!(tree.sections[0].path, "/Product");
    assert_eq!(tree.sections[1].path, "/Product/Notes");
    assert_eq!(tree.sections[2].path, "/Product/Todo");
    assert_eq!(tree.sections[3].path, "/Journal");
    assert_eq!(tree.sections[4].path, "/Journal/2026-04-06");
}

#[test]
fn test_parse_duplicate_headings() {
    let content = "# Notes\n\nFirst.\n\n# Notes\n\nSecond.\n";
    let tree = parse_document(content);

    assert_eq!(tree.sections.len(), 2);
    assert_eq!(tree.sections[0].path, "/Notes[1]");
    assert_eq!(tree.sections[1].path, "/Notes[2]");
}

#[test]
fn test_parse_frontmatter_and_preamble_and_sections() {
    let content = "---\ntitle: Full\n---\n\nPreamble text.\n\n# Heading\n\nContent.\n";
    let tree = parse_document(content);

    assert!(tree.frontmatter.is_some());
    assert!(tree.preamble.contains("Preamble text."));
    assert_eq!(tree.sections.len(), 1);
    assert_eq!(tree.sections[0].path, "/Heading");
}
