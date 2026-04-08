use std::collections::HashMap;

use crate::domain::section::Section;

/// Detect renames between two section maps using content similarity.
///
/// If a section path exists in `from` but not `to`, and a new path exists
/// in `to` with similar content (Jaccard word similarity > threshold),
/// returns the pair as a rename.
pub fn detect_renames(
    _from: &HashMap<String, Section>,
    _to: &HashMap<String, Section>,
) -> Vec<(String, String)> {
    // TODO: Implement in Phase 1
    Vec::new()
}

/// Compute Jaccard similarity between two strings based on word sets.
pub fn jaccard_word_similarity(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;

    let words_a: HashSet<&str> = a.split_whitespace().collect();
    let words_b: HashSet<&str> = b.split_whitespace().collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }
    if words_a.is_empty() || words_b.is_empty() {
        return 0.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    intersection as f64 / union as f64
}

#[cfg(test)]
mod section_diff_tests {
    use super::*;

    #[test]
    fn test_jaccard_identical() {
        assert!((jaccard_word_similarity("hello world", "hello world") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_jaccard_disjoint() {
        assert!((jaccard_word_similarity("hello world", "foo bar")).abs() < f64::EPSILON);
    }

    #[test]
    fn test_jaccard_partial() {
        let sim = jaccard_word_similarity("hello world foo", "hello world bar");
        assert!(sim > 0.4 && sim < 0.7);
    }

    #[test]
    fn test_jaccard_empty() {
        assert!((jaccard_word_similarity("", "") - 1.0).abs() < f64::EPSILON);
        assert!((jaccard_word_similarity("hello", "")).abs() < f64::EPSILON);
    }
}
