use super::*;
use crate::domain::section::{Frontmatter, FrontmatterFormat, Section, SectionTree};
use crate::markdown::parser::parse_document;
use std::collections::BTreeMap;

#[test]
fn test_reassemble_simple() {
    let tree = SectionTree {
        frontmatter: None,
        preamble: String::new(),
        sections: vec![Section {
            path: "/Hello".to_string(),
            heading_text: "Hello".to_string(),
            level: 1,
            content: "World.".to_string(),
            content_hash: Section::compute_hash("World."),
            ordinal: None,
            raw_source: None,
        }],
    };

    let output = reassemble(&tree);
    assert!(output.contains("# Hello"));
    assert!(output.contains("World."));
}

#[test]
fn test_reassemble_with_frontmatter() {
    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), "My Notes".to_string());

    let tree = SectionTree {
        frontmatter: Some(Frontmatter {
            format: FrontmatterFormat::Yaml,
            raw: "title: My Notes".to_string(),
            fields,
        }),
        preamble: String::new(),
        sections: vec![Section {
            path: "/Notes".to_string(),
            heading_text: "Notes".to_string(),
            level: 1,
            content: "Content here.".to_string(),
            content_hash: Section::compute_hash("Content here."),
            ordinal: None,
            raw_source: None,
        }],
    };

    let output = reassemble(&tree);
    assert!(output.starts_with("---\n"));
    assert!(output.contains("title: My Notes"));
    assert!(output.contains("# Notes"));
}

#[test]
fn test_reassemble_with_preamble() {
    let tree = SectionTree {
        frontmatter: None,
        preamble: "Some preamble text.".to_string(),
        sections: vec![Section {
            path: "/Heading".to_string(),
            heading_text: "Heading".to_string(),
            level: 1,
            content: "Body.".to_string(),
            content_hash: Section::compute_hash("Body."),
            ordinal: None,
            raw_source: None,
        }],
    };

    let output = reassemble(&tree);
    assert!(output.contains("Some preamble text."));
    assert!(output.contains("# Heading"));
}

#[test]
fn test_reassemble_multiple_levels() {
    let tree = SectionTree {
        frontmatter: None,
        preamble: String::new(),
        sections: vec![
            Section {
                path: "/Product".to_string(),
                heading_text: "Product".to_string(),
                level: 1,
                content: "".to_string(),
                content_hash: Section::compute_hash(""),
                ordinal: None,
                raw_source: None,
            },
            Section {
                path: "/Product/Notes".to_string(),
                heading_text: "Notes".to_string(),
                level: 2,
                content: "My notes.".to_string(),
                content_hash: Section::compute_hash("My notes."),
                ordinal: None,
                raw_source: None,
            },
        ],
    };

    let output = reassemble(&tree);
    assert!(output.contains("# Product"));
    assert!(output.contains("## Notes"));
    assert!(output.contains("My notes."));
}

#[test]
fn test_reassemble_empty_document() {
    let tree = SectionTree {
        frontmatter: None,
        preamble: String::new(),
        sections: Vec::new(),
    };

    let output = reassemble(&tree);
    assert!(output.is_empty());
}

#[test]
fn test_reassemble_toml_frontmatter() {
    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), "TOML Doc".to_string());

    let tree = SectionTree {
        frontmatter: Some(Frontmatter {
            format: FrontmatterFormat::Toml,
            raw: "title = \"TOML Doc\"".to_string(),
            fields,
        }),
        preamble: String::new(),
        sections: Vec::new(),
    };

    let output = reassemble(&tree);
    assert!(output.starts_with("+++\n"));
    assert!(output.contains("title = \"TOML Doc\""));
    assert!(output.contains("+++"));
}

#[test]
fn test_roundtrip_simple() {
    let input = "# Hello\n\nWorld.\n\n## Sub\n\nSub content.\n";
    let tree = parse_document(input);
    let output = reassemble(&tree);
    assert_eq!(output, input);
}

#[test]
fn test_roundtrip_with_preamble() {
    let input = "Some preamble text.\n\n# Heading\n\nBody.\n";
    let tree = parse_document(input);
    let output = reassemble(&tree);
    assert_eq!(output, input);
}

#[test]
fn test_roundtrip_with_frontmatter() {
    let input = "---\ntitle: My Notes\n---\n\n# Notes\n\nSome notes.\n";
    let tree = parse_document(input);
    let output = reassemble(&tree);
    assert_eq!(output, input);
}

#[test]
fn test_roundtrip_multiple_h1s() {
    let input = "# Product\n\nProduct notes.\n\n# Journal\n\nJournal entry.\n";
    let tree = parse_document(input);
    let output = reassemble(&tree);
    assert_eq!(output, input);
}

#[test]
fn test_roundtrip_empty_section() {
    let input = "# Product\n\n## Notes\n\nMy notes.\n";
    let tree = parse_document(input);
    let output = reassemble(&tree);
    assert_eq!(output, input);
}

#[test]
fn test_roundtrip_frontmatter_preamble_and_sections() {
    let input = "---\ntitle: Full\n---\n\nPreamble text.\n\n# Heading\n\nContent.\n";
    let tree = parse_document(input);
    let output = reassemble(&tree);
    assert_eq!(output, input);
}
