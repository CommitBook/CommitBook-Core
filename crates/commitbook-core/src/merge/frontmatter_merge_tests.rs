use super::*;
use crate::domain::section::{Frontmatter, FrontmatterFormat};
use std::collections::BTreeMap;

fn make_fm(pairs: &[(&str, &str)]) -> Option<Frontmatter> {
    let fields: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let raw = fields
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(Frontmatter {
        format: FrontmatterFormat::Yaml,
        raw,
        fields,
    })
}

#[test]
fn test_no_frontmatter() {
    let (result, conflicts) = merge_frontmatter(&None, &None, &None);
    assert!(result.is_none());
    assert!(conflicts.is_empty());
}

#[test]
fn test_unchanged_frontmatter() {
    let base = make_fm(&[("title", "Hello"), ("author", "Manuel")]);
    let local = make_fm(&[("title", "Hello"), ("author", "Manuel")]);
    let remote = make_fm(&[("title", "Hello"), ("author", "Manuel")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    let fm = result.unwrap();
    assert_eq!(fm.fields.get("title").unwrap(), "Hello");
    assert_eq!(fm.fields.get("author").unwrap(), "Manuel");
}

#[test]
fn test_local_only_change() {
    let base = make_fm(&[("title", "Hello")]);
    let local = make_fm(&[("title", "Updated")]);
    let remote = make_fm(&[("title", "Hello")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    assert_eq!(result.unwrap().fields.get("title").unwrap(), "Updated");
}

#[test]
fn test_remote_only_change() {
    let base = make_fm(&[("title", "Hello")]);
    let local = make_fm(&[("title", "Hello")]);
    let remote = make_fm(&[("title", "Updated")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    assert_eq!(result.unwrap().fields.get("title").unwrap(), "Updated");
}

#[test]
fn test_different_keys_changed() {
    let base = make_fm(&[("title", "Hello"), ("author", "Manuel")]);
    let local = make_fm(&[("title", "Updated"), ("author", "Manuel")]);
    let remote = make_fm(&[("title", "Hello"), ("author", "New Author")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    let fm = result.unwrap();
    assert_eq!(fm.fields.get("title").unwrap(), "Updated");
    assert_eq!(fm.fields.get("author").unwrap(), "New Author");
}

#[test]
fn test_same_key_conflict() {
    let base = make_fm(&[("title", "Original")]);
    let local = make_fm(&[("title", "Local Title")]);
    let remote = make_fm(&[("title", "Remote Title")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].conflict_type, ConflictType::FrontmatterConflict);
    assert!(conflicts[0].local_content.contains("Local Title"));
    assert!(conflicts[0].remote_content.contains("Remote Title"));
    // Merged result keeps local value.
    assert_eq!(result.unwrap().fields.get("title").unwrap(), "Local Title");
}

#[test]
fn test_key_added_locally() {
    let base = make_fm(&[("title", "Hello")]);
    let local = make_fm(&[("title", "Hello"), ("tags", "rust")]);
    let remote = make_fm(&[("title", "Hello")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    let fm = result.unwrap();
    assert_eq!(fm.fields.get("tags").unwrap(), "rust");
}

#[test]
fn test_key_added_remotely() {
    let base = make_fm(&[("title", "Hello")]);
    let local = make_fm(&[("title", "Hello")]);
    let remote = make_fm(&[("title", "Hello"), ("tags", "rust")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    let fm = result.unwrap();
    assert_eq!(fm.fields.get("tags").unwrap(), "rust");
}

#[test]
fn test_key_added_both_same_value() {
    let base: Option<Frontmatter> = None;
    let local = make_fm(&[("title", "Same")]);
    let remote = make_fm(&[("title", "Same")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    assert_eq!(result.unwrap().fields.get("title").unwrap(), "Same");
}

#[test]
fn test_key_added_both_different_values() {
    let base: Option<Frontmatter> = None;
    let local = make_fm(&[("title", "Local")]);
    let remote = make_fm(&[("title", "Remote")]);

    let (_result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert_eq!(conflicts.len(), 1);
}

#[test]
fn test_key_deleted_locally_unchanged_remotely() {
    let base = make_fm(&[("title", "Hello"), ("author", "Manuel")]);
    let local = make_fm(&[("title", "Hello")]); // author deleted
    let remote = make_fm(&[("title", "Hello"), ("author", "Manuel")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    let fm = result.unwrap();
    assert!(!fm.fields.contains_key("author")); // deletion preserved
}

#[test]
fn test_key_deleted_locally_changed_remotely() {
    let base = make_fm(&[("title", "Hello"), ("author", "Manuel")]);
    let local = make_fm(&[("title", "Hello")]); // author deleted
    let remote = make_fm(&[("title", "Hello"), ("author", "New Author")]); // author changed

    let (_result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].local_content.contains("(deleted)"));
}

#[test]
fn test_both_sides_change_to_same_value() {
    let base = make_fm(&[("title", "Old")]);
    let local = make_fm(&[("title", "New")]);
    let remote = make_fm(&[("title", "New")]);

    let (result, conflicts) = merge_frontmatter(&base, &local, &remote);
    assert!(conflicts.is_empty());
    assert_eq!(result.unwrap().fields.get("title").unwrap(), "New");
}
