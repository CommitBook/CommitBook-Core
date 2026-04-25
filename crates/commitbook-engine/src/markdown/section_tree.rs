use std::collections::HashMap;

use crate::domain::section::Section;

/// Build section paths from a flat list of (level, heading_text, content) tuples.
/// Handles heading hierarchy and duplicate sibling disambiguation with ordinals.
pub fn build_sections(raw_sections: Vec<(u8, String, String)>) -> Vec<Section> {
    // Stack tracks the current heading path: Vec<(level, text)>
    let mut stack: Vec<(u8, String)> = Vec::new();
    // Track ordinals for duplicate siblings under the same parent path.
    // Key: (parent_path, heading_text), Value: count of occurrences so far.
    let mut sibling_counts: HashMap<(String, String), u32> = HashMap::new();
    let mut sections = Vec::new();

    for (level, heading_text, content) in raw_sections {
        // Pop the stack back to the parent level.
        while let Some(top) = stack.last() {
            if top.0 >= level {
                stack.pop();
            } else {
                break;
            }
        }

        // Build parent path from current stack.
        let parent_path = if stack.is_empty() {
            String::new()
        } else {
            stack
                .iter()
                .map(|(_, text)| text.as_str())
                .collect::<Vec<_>>()
                .join("/")
                .insert_str_prefix("/")
        };

        // Track duplicate siblings.
        let sibling_key = (parent_path.clone(), heading_text.clone());
        let count = sibling_counts.entry(sibling_key).or_insert(0);
        *count += 1;

        // Build the section path.
        let ordinal = if *count > 1 { Some(*count) } else { None };
        let path = if ordinal.is_some() {
            format!("{}/{heading_text}[{count}]", parent_path)
        } else {
            format!("{}/{heading_text}", parent_path)
        };

        let content_hash = Section::compute_hash(&content);

        sections.push(Section {
            path,
            heading_text: heading_text.clone(),
            level,
            content,
            content_hash,
            ordinal,
            raw_source: None,
        });

        stack.push((level, heading_text));
    }

    // Post-process: if any heading has count > 1, retroactively add [1] to the first occurrence.
    let mut first_occurrence_map: HashMap<String, usize> = HashMap::new();
    let mut needs_retroactive: HashMap<String, bool> = HashMap::new();

    for (i, section) in sections.iter().enumerate() {
        let base_key = format!(
            "{}/{}",
            section
                .path
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or(""),
            section.heading_text
        );
        first_occurrence_map.entry(base_key.clone()).or_insert(i);

        // Check if the parent_path + heading combo has duplicates.
        let parent = section
            .path
            .rsplit_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_default();
        let dup_key = format!("{parent}|{}", section.heading_text);
        if section.ordinal.is_some() {
            needs_retroactive.insert(dup_key, true);
        }
    }

    for key in needs_retroactive.keys() {
        let parts: Vec<&str> = key.splitn(2, '|').collect();
        if parts.len() == 2 {
            let parent = parts[0];
            let heading = parts[1];
            let base_path = format!("{parent}/{heading}");
            if let Some(&idx) = first_occurrence_map.get(&base_path) {
                if sections[idx].ordinal.is_none() {
                    sections[idx].ordinal = Some(1);
                    sections[idx].path = format!("{parent}/{heading}[1]");
                }
            }
        }
    }

    sections
}

/// Helper trait for inserting a prefix into a String.
trait InsertStrPrefix {
    fn insert_str_prefix(self, prefix: &str) -> String;
}

impl InsertStrPrefix for String {
    fn insert_str_prefix(self, prefix: &str) -> String {
        if self.starts_with(prefix) {
            self
        } else {
            format!("{prefix}{self}")
        }
    }
}

#[cfg(test)]
#[path = "section_tree_tests.rs"]
mod tests;
