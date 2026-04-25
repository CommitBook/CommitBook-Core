use std::collections::{HashMap, HashSet};

use crate::domain::section::Section;

/// Threshold for Jaccard word similarity to consider a section as renamed.
const RENAME_SIMILARITY_THRESHOLD: f64 = 0.6;

/// Detect renames between two section maps using content similarity.
///
/// If a section path exists in `from` but not `to`, and a new path exists
/// in `to` with similar content (Jaccard word similarity > threshold),
/// returns the pair as a rename: (old_path, new_path).
pub fn detect_renames<'a>(
    from: &HashMap<&'a str, &'a Section>,
    to: &HashMap<&'a str, &'a Section>,
) -> Vec<(String, String)> {
    // Sections removed from `from` (exist in from, not in to).
    let removed: Vec<&str> = from
        .keys()
        .filter(|k| !to.contains_key(*k))
        .copied()
        .collect();

    // Sections added in `to` (exist in to, not in from).
    let added: Vec<&str> = to
        .keys()
        .filter(|k| !from.contains_key(*k))
        .copied()
        .collect();

    if removed.is_empty() || added.is_empty() {
        return Vec::new();
    }

    let mut renames = Vec::new();
    let mut used_added: HashSet<&str> = HashSet::new();

    for &removed_path in &removed {
        let removed_section = &from[removed_path];
        let mut best_match: Option<(&str, f64)> = None;

        for &added_path in &added {
            if used_added.contains(added_path) {
                continue;
            }
            let added_section = &to[added_path];

            // Only compare sections at the same heading level.
            if removed_section.level != added_section.level {
                continue;
            }

            let similarity = jaccard_word_similarity(
                &removed_section.content,
                &added_section.content,
            );

            if similarity >= RENAME_SIMILARITY_THRESHOLD {
                if let Some((_, best_sim)) = best_match {
                    if similarity > best_sim {
                        best_match = Some((added_path, similarity));
                    }
                } else {
                    best_match = Some((added_path, similarity));
                }
            }
        }

        if let Some((added_path, _)) = best_match {
            renames.push((removed_path.to_string(), added_path.to_string()));
            used_added.insert(added_path);
        }
    }

    renames
}

/// Compute Jaccard similarity between two strings based on word sets.
pub fn jaccard_word_similarity(a: &str, b: &str) -> f64 {
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
#[path = "section_diff_tests.rs"]
mod tests;
