use super::*;
use crate::domain::conflict::ConflictType;
use crate::domain::section::{Section, SectionTree};

fn make_section(path: &str, level: u8, heading: &str, content: &str) -> Section {
    Section {
        path: path.to_string(),
        heading_text: heading.to_string(),
        level,
        content: content.to_string(),
        content_hash: Section::compute_hash(content),
        ordinal: None,
    }
}

fn make_tree(sections: Vec<Section>) -> SectionTree {
    SectionTree {
        frontmatter: None,
        preamble: String::new(),
        sections,
    }
}

// --- Case A: unchanged locally, changed remotely → take remote ---

#[test]
fn test_case_a_unchanged_local_changed_remote() {
    let base = make_tree(vec![make_section("/Notes", 1, "Notes", "Original content")]);
    let local = make_tree(vec![make_section("/Notes", 1, "Notes", "Original content")]);
    let remote = make_tree(vec![make_section("/Notes", 1, "Notes", "Updated by remote")]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.auto_resolved_count, 1);
    assert_eq!(result.merged_tree.sections.len(), 1);
    assert_eq!(result.merged_tree.sections[0].content, "Updated by remote");
}

// --- Case B: changed locally, unchanged remotely → take local ---

#[test]
fn test_case_b_changed_local_unchanged_remote() {
    let base = make_tree(vec![make_section("/Notes", 1, "Notes", "Original content")]);
    let local = make_tree(vec![make_section("/Notes", 1, "Notes", "Updated by local")]);
    let remote = make_tree(vec![make_section("/Notes", 1, "Notes", "Original content")]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.auto_resolved_count, 1);
    assert_eq!(result.merged_tree.sections[0].content, "Updated by local");
}

// --- Case C: changed on different section paths → auto-merge both ---

#[test]
fn test_case_c_different_sections_changed() {
    let base = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Notes content"),
        make_section("/Todo", 1, "Todo", "Todo content"),
    ]);
    let local = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Notes updated locally"),
        make_section("/Todo", 1, "Todo", "Todo content"),
    ]);
    let remote = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Notes content"),
        make_section("/Todo", 1, "Todo", "Todo updated remotely"),
    ]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.auto_resolved_count, 2);
    assert_eq!(result.merged_tree.sections.len(), 2);
    assert_eq!(result.merged_tree.sections[0].content, "Notes updated locally");
    assert_eq!(result.merged_tree.sections[1].content, "Todo updated remotely");
}

// --- Case D: changed on same section path → append-both conflict ---

#[test]
fn test_case_d_same_section_both_changed() {
    let base = make_tree(vec![make_section("/Notes", 1, "Notes", "Original")]);
    let local = make_tree(vec![make_section("/Notes", 1, "Notes", "Local version")]);
    let remote = make_tree(vec![make_section("/Notes", 1, "Notes", "Remote version")]);

    let result = merge_document(&base, &local, &remote);
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].conflict_type, ConflictType::SectionConflict);
    assert_eq!(result.conflicts[0].section_path.as_deref(), Some("/Notes"));
    assert_eq!(result.conflicts[0].local_content, "Local version");
    assert_eq!(result.conflicts[0].remote_content, "Remote version");

    // Merged content should have append-both format.
    let merged = &result.merged_tree.sections[0].content;
    assert!(merged.contains("Local version"));
    assert!(merged.contains("Remote version"));
    assert!(merged.contains("---"));
}

// --- Case E: section added locally only → keep ---

#[test]
fn test_case_e_added_locally() {
    let base = make_tree(vec![make_section("/Notes", 1, "Notes", "Existing")]);
    let local = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Existing"),
        make_section("/New", 1, "New", "Brand new section"),
    ]);
    let remote = make_tree(vec![make_section("/Notes", 1, "Notes", "Existing")]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree.sections.len(), 2);
    assert!(result.merged_tree.sections.iter().any(|s| s.path == "/New"));
}

// --- Case F: section added remotely only → keep ---

#[test]
fn test_case_f_added_remotely() {
    let base = make_tree(vec![make_section("/Notes", 1, "Notes", "Existing")]);
    let local = make_tree(vec![make_section("/Notes", 1, "Notes", "Existing")]);
    let remote = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Existing"),
        make_section("/New", 1, "New", "Remote new section"),
    ]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree.sections.len(), 2);
    let new_section = result.merged_tree.sections.iter().find(|s| s.path == "/New").unwrap();
    assert_eq!(new_section.content, "Remote new section");
}

// --- Case G: delete vs edit → conflict ---

#[test]
fn test_case_g_deleted_locally_edited_remotely() {
    let base = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Original"),
        make_section("/Todo", 1, "Todo", "Todo content"),
    ]);
    let local = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Original"),
        // /Todo deleted locally
    ]);
    let remote = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Original"),
        make_section("/Todo", 1, "Todo", "Todo edited remotely"),
    ]);

    let result = merge_document(&base, &local, &remote);
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].section_path.as_deref(), Some("/Todo"));
    assert!(result.conflicts[0].local_content.contains("(deleted)"));
    assert_eq!(result.conflicts[0].remote_content, "Todo edited remotely");
}

#[test]
fn test_case_g_deleted_remotely_edited_locally() {
    let base = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Original"),
        make_section("/Todo", 1, "Todo", "Todo content"),
    ]);
    let local = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Original"),
        make_section("/Todo", 1, "Todo", "Todo edited locally"),
    ]);
    let remote = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Original"),
        // /Todo deleted remotely
    ]);

    let result = merge_document(&base, &local, &remote);
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].local_content, "Todo edited locally");
    assert!(result.conflicts[0].remote_content.contains("(deleted)"));
}

// --- Delete on both sides → no conflict ---

#[test]
fn test_deleted_on_both_sides() {
    let base = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Content"),
        make_section("/Old", 1, "Old", "Old content"),
    ]);
    let local = make_tree(vec![make_section("/Notes", 1, "Notes", "Content")]);
    let remote = make_tree(vec![make_section("/Notes", 1, "Notes", "Content")]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree.sections.len(), 1);
    assert_eq!(result.merged_tree.sections[0].path, "/Notes");
}

// --- Delete vs unchanged → honor deletion ---

#[test]
fn test_deleted_locally_unchanged_remotely() {
    let base = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Content"),
        make_section("/Old", 1, "Old", "Old content"),
    ]);
    let local = make_tree(vec![make_section("/Notes", 1, "Notes", "Content")]);
    let remote = make_tree(vec![
        make_section("/Notes", 1, "Notes", "Content"),
        make_section("/Old", 1, "Old", "Old content"),
    ]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree.sections.len(), 1); // /Old deleted
}

// --- Both changed to same content → no conflict ---

#[test]
fn test_both_changed_to_same_value() {
    let base = make_tree(vec![make_section("/Notes", 1, "Notes", "Original")]);
    let local = make_tree(vec![make_section("/Notes", 1, "Notes", "Same update")]);
    let remote = make_tree(vec![make_section("/Notes", 1, "Notes", "Same update")]);

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree.sections[0].content, "Same update");
}

// --- Preamble merge ---

#[test]
fn test_preamble_changed_locally_only() {
    let base = SectionTree {
        frontmatter: None,
        preamble: "Old preamble".to_string(),
        sections: vec![make_section("/A", 1, "A", "content")],
    };
    let local = SectionTree {
        frontmatter: None,
        preamble: "New preamble".to_string(),
        sections: vec![make_section("/A", 1, "A", "content")],
    };
    let remote = SectionTree {
        frontmatter: None,
        preamble: "Old preamble".to_string(),
        sections: vec![make_section("/A", 1, "A", "content")],
    };

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree.preamble, "New preamble");
}

#[test]
fn test_preamble_conflict() {
    let base = SectionTree {
        frontmatter: None,
        preamble: "Original".to_string(),
        sections: Vec::new(),
    };
    let local = SectionTree {
        frontmatter: None,
        preamble: "Local preamble".to_string(),
        sections: Vec::new(),
    };
    let remote = SectionTree {
        frontmatter: None,
        preamble: "Remote preamble".to_string(),
        sections: Vec::new(),
    };

    let result = merge_document(&base, &local, &remote);
    assert_eq!(result.conflicts.len(), 1);
    assert_eq!(result.conflicts[0].section_path.as_deref(), Some("/_preamble"));
}

// --- Empty documents ---

#[test]
fn test_merge_empty_documents() {
    let base = make_tree(Vec::new());
    let local = make_tree(Vec::new());
    let remote = make_tree(Vec::new());

    let result = merge_document(&base, &local, &remote);
    assert!(result.conflicts.is_empty());
    assert!(result.merged_tree.sections.is_empty());
}

// --- Complex: multiple operations ---

#[test]
fn test_complex_multi_section_merge() {
    let base = make_tree(vec![
        make_section("/Product", 1, "Product", "Product info"),
        make_section("/Product/Notes", 2, "Notes", "Old notes"),
        make_section("/Product/Todo", 2, "Todo", "Old todos"),
        make_section("/Journal", 1, "Journal", "Journal entries"),
    ]);
    let local = make_tree(vec![
        make_section("/Product", 1, "Product", "Product info"),
        make_section("/Product/Notes", 2, "Notes", "Updated notes locally"),
        make_section("/Product/Todo", 2, "Todo", "Old todos"),
        make_section("/Journal", 1, "Journal", "Journal entries"),
        make_section("/Ideas", 1, "Ideas", "New ideas section"),
    ]);
    let remote = make_tree(vec![
        make_section("/Product", 1, "Product", "Product info"),
        make_section("/Product/Notes", 2, "Notes", "Old notes"),
        make_section("/Product/Todo", 2, "Todo", "Updated todos remotely"),
        make_section("/Journal", 1, "Journal", "Updated journal remotely"),
    ]);

    let result = merge_document(&base, &local, &remote);

    // No conflicts: Notes changed local only, Todo changed remote only,
    // Product unchanged, Journal changed remote only, Ideas added locally.
    assert!(result.conflicts.is_empty());
    assert_eq!(result.merged_tree.sections.len(), 5);

    let notes = result.merged_tree.sections.iter().find(|s| s.path == "/Product/Notes").unwrap();
    assert_eq!(notes.content, "Updated notes locally");

    let todo = result.merged_tree.sections.iter().find(|s| s.path == "/Product/Todo").unwrap();
    assert_eq!(todo.content, "Updated todos remotely");

    let journal = result.merged_tree.sections.iter().find(|s| s.path == "/Journal").unwrap();
    assert_eq!(journal.content, "Updated journal remotely");

    assert!(result.merged_tree.sections.iter().any(|s| s.path == "/Ideas"));
}
