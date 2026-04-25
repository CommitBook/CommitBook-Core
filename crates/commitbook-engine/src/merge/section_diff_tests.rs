use super::*;
use crate::domain::section::Section;

fn make_section(path: &str, level: u8, content: &str) -> Section {
    Section {
        path: path.to_string(),
        heading_text: path.rsplit('/').next().unwrap_or(path).to_string(),
        level,
        content: content.to_string(),
        content_hash: Section::compute_hash(content),
        ordinal: None,
        raw_source: None,
    }
}

#[test]
fn test_jaccard_identical() {
    assert!((jaccard_word_similarity("hello world", "hello world") - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_jaccard_disjoint() {
    assert!(jaccard_word_similarity("hello world", "foo bar").abs() < f64::EPSILON);
}

#[test]
fn test_jaccard_partial() {
    let sim = jaccard_word_similarity("hello world foo", "hello world bar");
    assert!((sim - 0.5).abs() < f64::EPSILON);
}

#[test]
fn test_jaccard_empty() {
    assert!((jaccard_word_similarity("", "") - 1.0).abs() < f64::EPSILON);
    assert!(jaccard_word_similarity("hello", "").abs() < f64::EPSILON);
}

#[test]
fn test_detect_renames_simple() {
    let old_section = make_section("/Notes", 1, "This is my important note about Rust programming and memory safety");
    let new_section = make_section("/My Notes", 1, "This is my important note about Rust programming and memory safety");

    let from: HashMap<&str, &Section> = [("/Notes", &old_section)].into();
    let to: HashMap<&str, &Section> = [("/My Notes", &new_section)].into();

    let renames = detect_renames(&from, &to);
    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0].0, "/Notes");
    assert_eq!(renames[0].1, "/My Notes");
}

#[test]
fn test_detect_renames_no_match_different_content() {
    let old_section = make_section("/Notes", 1, "Completely different content here");
    let new_section = make_section("/My Notes", 1, "Totally unrelated text about something else entirely");

    let from: HashMap<&str, &Section> = [("/Notes", &old_section)].into();
    let to: HashMap<&str, &Section> = [("/My Notes", &new_section)].into();

    let renames = detect_renames(&from, &to);
    assert!(renames.is_empty());
}

#[test]
fn test_detect_renames_different_levels_no_match() {
    let old_section = make_section("/Notes", 1, "Same content here for testing");
    let new_section = make_section("/DeepNotes", 2, "Same content here for testing");

    let from: HashMap<&str, &Section> = [("/Notes", &old_section)].into();
    let to: HashMap<&str, &Section> = [("/DeepNotes", &new_section)].into();

    let renames = detect_renames(&from, &to);
    assert!(renames.is_empty()); // Different levels, no rename detected.
}

#[test]
fn test_detect_renames_no_removed_sections() {
    let s1 = make_section("/A", 1, "content");
    let s2 = make_section("/A", 1, "content");
    let s3 = make_section("/B", 1, "other");

    let from: HashMap<&str, &Section> = [("/A", &s1)].into();
    let to: HashMap<&str, &Section> = [("/A", &s2), ("/B", &s3)].into();

    let renames = detect_renames(&from, &to);
    assert!(renames.is_empty());
}

#[test]
fn test_detect_renames_multiple() {
    let old_a = make_section("/Notes", 1, "Notes about programming in Rust with async await");
    let old_b = make_section("/Todo", 1, "Buy groceries and clean the house this weekend");
    let new_a = make_section("/My Notes", 1, "Notes about programming in Rust with async await patterns");
    let new_b = make_section("/Tasks", 1, "Buy groceries and clean the house this weekend please");

    let from: HashMap<&str, &Section> = [
        ("/Notes", &old_a),
        ("/Todo", &old_b),
    ].into();
    let to: HashMap<&str, &Section> = [
        ("/My Notes", &new_a),
        ("/Tasks", &new_b),
    ].into();

    let renames = detect_renames(&from, &to);
    assert_eq!(renames.len(), 2);
}
