use super::*;

#[test]
fn test_single_heading() {
    let raw = vec![(1, "Product".to_string(), "Some notes.".to_string())];
    let sections = build_sections(raw);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].path, "/Product");
    assert_eq!(sections[0].level, 1);
    assert_eq!(sections[0].content, "Some notes.");
    assert!(sections[0].ordinal.is_none());
}

#[test]
fn test_nested_headings() {
    let raw = vec![
        (1, "Product".to_string(), "Intro.".to_string()),
        (2, "Notes".to_string(), "My notes.".to_string()),
        (2, "Todo".to_string(), "My todos.".to_string()),
    ];
    let sections = build_sections(raw);
    assert_eq!(sections.len(), 3);
    assert_eq!(sections[0].path, "/Product");
    assert_eq!(sections[1].path, "/Product/Notes");
    assert_eq!(sections[2].path, "/Product/Todo");
}

#[test]
fn test_sibling_h1s() {
    let raw = vec![
        (1, "Product".to_string(), "".to_string()),
        (1, "Journal".to_string(), "".to_string()),
    ];
    let sections = build_sections(raw);
    assert_eq!(sections[0].path, "/Product");
    assert_eq!(sections[1].path, "/Journal");
}

#[test]
fn test_duplicate_siblings() {
    let raw = vec![
        (1, "Notes".to_string(), "First.".to_string()),
        (1, "Notes".to_string(), "Second.".to_string()),
    ];
    let sections = build_sections(raw);
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].path, "/Notes[1]");
    assert_eq!(sections[0].ordinal, Some(1));
    assert_eq!(sections[1].path, "/Notes[2]");
    assert_eq!(sections[1].ordinal, Some(2));
}

#[test]
fn test_duplicate_nested_siblings() {
    let raw = vec![
        (1, "Product".to_string(), "".to_string()),
        (2, "Notes".to_string(), "First.".to_string()),
        (2, "Notes".to_string(), "Second.".to_string()),
    ];
    let sections = build_sections(raw);
    assert_eq!(sections[0].path, "/Product");
    assert_eq!(sections[1].path, "/Product/Notes[1]");
    assert_eq!(sections[2].path, "/Product/Notes[2]");
}

#[test]
fn test_deep_nesting() {
    let raw = vec![
        (1, "Root".to_string(), "".to_string()),
        (2, "Child".to_string(), "".to_string()),
        (3, "Grandchild".to_string(), "Deep content.".to_string()),
    ];
    let sections = build_sections(raw);
    assert_eq!(sections[0].path, "/Root");
    assert_eq!(sections[1].path, "/Root/Child");
    assert_eq!(sections[2].path, "/Root/Child/Grandchild");
}

#[test]
fn test_heading_level_jump_back() {
    // H1 -> H3 -> H1 (skip H2, then pop back)
    let raw = vec![
        (1, "First".to_string(), "".to_string()),
        (3, "Deep".to_string(), "".to_string()),
        (1, "Second".to_string(), "".to_string()),
    ];
    let sections = build_sections(raw);
    assert_eq!(sections[0].path, "/First");
    assert_eq!(sections[1].path, "/First/Deep");
    assert_eq!(sections[2].path, "/Second");
}

#[test]
fn test_content_hashes_differ() {
    let raw = vec![
        (1, "A".to_string(), "Content A.".to_string()),
        (1, "B".to_string(), "Content B.".to_string()),
    ];
    let sections = build_sections(raw);
    assert_ne!(sections[0].content_hash, sections[1].content_hash);
}

#[test]
fn test_empty_sections() {
    let raw: Vec<(u8, String, String)> = Vec::new();
    let sections = build_sections(raw);
    assert!(sections.is_empty());
}
