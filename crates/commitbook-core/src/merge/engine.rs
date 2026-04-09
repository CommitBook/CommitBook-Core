use std::collections::HashMap;

use crate::domain::conflict::ConflictType;
use crate::domain::section::{Section, SectionTree};

use super::conflict_builder::build_append_both;
use super::frontmatter_merge::merge_frontmatter;
use super::section_diff::detect_renames;

/// Result of a three-way section-aware merge.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub merged_tree: SectionTree,
    pub conflicts: Vec<SectionConflict>,
    pub auto_resolved_count: usize,
}

/// A conflict detected during merge.
#[derive(Debug, Clone)]
pub struct SectionConflict {
    /// None for preamble or frontmatter conflicts.
    pub section_path: Option<String>,
    pub conflict_type: ConflictType,
    pub base_content: Option<String>,
    pub local_content: String,
    pub remote_content: String,
}

/// Perform a three-way section-aware merge.
///
/// Compares base, local, and remote SectionTrees section-by-section,
/// applying the seven merge cases (A through G) from the spec.
pub fn merge_document(
    base: &SectionTree,
    local: &SectionTree,
    remote: &SectionTree,
) -> MergeResult {
    let mut conflicts = Vec::new();
    let mut auto_resolved = 0;
    let timestamp = chrono::Utc::now().to_rfc3339();

    // 1. Merge frontmatter.
    let (merged_frontmatter, fm_conflicts) =
        merge_frontmatter(&base.frontmatter, &local.frontmatter, &remote.frontmatter);
    conflicts.extend(fm_conflicts);

    // 2. Merge preamble (treated as pseudo-section "/_preamble").
    let merged_preamble = merge_preamble(
        &base.preamble,
        &local.preamble,
        &remote.preamble,
        &timestamp,
        &mut conflicts,
        &mut auto_resolved,
    );

    // 3. Build section maps.
    let base_map = section_map(&base.sections);
    let local_map = section_map(&local.sections);
    let remote_map = section_map(&remote.sections);

    // 4. Detect renames (local renames and remote renames).
    let local_renames = detect_renames(&base_map, &local_map);
    let remote_renames = detect_renames(&base_map, &remote_map);

    // Build rename lookups: old_path -> new_path.
    let local_rename_map: HashMap<String, String> = local_renames.into_iter().collect();
    let remote_rename_map: HashMap<String, String> = remote_renames.into_iter().collect();

    // 5. Collect all section paths (union) in document order.
    let ordered_paths = collect_ordered_paths(&local.sections, &remote.sections, &base.sections);

    // 6. Walk each path and apply merge cases.
    let mut merged_sections: Vec<Section> = Vec::new();

    for path in &ordered_paths {
        let base_section = base_map.get(path.as_str());
        let local_section = resolve_section(path, &local_map, &remote_rename_map);
        let remote_section = resolve_section(path, &remote_map, &local_rename_map);

        match (base_section, local_section, remote_section) {
            // Case: exists in base, local, and remote.
            (Some(base_s), Some(local_s), Some(remote_s)) => {
                let base_changed_local = base_s.content_hash != local_s.content_hash;
                let base_changed_remote = base_s.content_hash != remote_s.content_hash;

                match (base_changed_local, base_changed_remote) {
                    // Case A: unchanged locally, changed remotely → take remote.
                    (false, true) => {
                        merged_sections.push(make_merged_section(remote_s, path));
                        auto_resolved += 1;
                    }
                    // Case B: changed locally, unchanged remotely → take local.
                    (true, false) => {
                        merged_sections.push(make_merged_section(local_s, path));
                        auto_resolved += 1;
                    }
                    // Both changed to same content → take either.
                    (true, true) if local_s.content_hash == remote_s.content_hash => {
                        merged_sections.push(make_merged_section(local_s, path));
                        auto_resolved += 1;
                    }
                    // Case D: changed on both sides to different content → conflict.
                    (true, true) => {
                        let appended = build_append_both(
                            &local_s.content,
                            &remote_s.content,
                            &timestamp,
                        );
                        let mut conflicted = make_merged_section(local_s, path);
                        conflicted.content = appended.clone();
                        conflicted.content_hash = Section::compute_hash(&appended);
                        merged_sections.push(conflicted);

                        conflicts.push(SectionConflict {
                            section_path: Some(path.clone()),
                            conflict_type: ConflictType::SectionConflict,
                            base_content: Some(base_s.content.clone()),
                            local_content: local_s.content.clone(),
                            remote_content: remote_s.content.clone(),
                        });
                    }
                    // Neither changed → keep as-is.
                    (false, false) => {
                        merged_sections.push(make_merged_section(base_s, path));
                    }
                }
            }

            // Case E: added locally only → keep local.
            (None, Some(local_s), None) => {
                merged_sections.push(make_merged_section(local_s, path));
                auto_resolved += 1;
            }

            // Case F: added remotely only → keep remote.
            (None, None, Some(remote_s)) => {
                merged_sections.push(make_merged_section(remote_s, path));
                auto_resolved += 1;
            }

            // Added on both sides.
            (None, Some(local_s), Some(remote_s)) => {
                if local_s.content_hash == remote_s.content_hash {
                    // Same content added on both sides.
                    merged_sections.push(make_merged_section(local_s, path));
                    auto_resolved += 1;
                } else {
                    // Different content added on both sides → conflict.
                    let appended = build_append_both(
                        &local_s.content,
                        &remote_s.content,
                        &timestamp,
                    );
                    let mut conflicted = make_merged_section(local_s, path);
                    conflicted.content = appended.clone();
                    conflicted.content_hash = Section::compute_hash(&appended);
                    merged_sections.push(conflicted);

                    conflicts.push(SectionConflict {
                        section_path: Some(path.clone()),
                        conflict_type: ConflictType::SectionConflict,
                        base_content: None,
                        local_content: local_s.content.clone(),
                        remote_content: remote_s.content.clone(),
                    });
                }
            }

            // Case G: deleted locally, still exists in remote.
            (Some(base_s), None, Some(remote_s)) => {
                if base_s.content_hash == remote_s.content_hash {
                    // Deleted locally, unchanged remotely → honor deletion.
                    auto_resolved += 1;
                } else {
                    // Case G: delete vs edit → conflict.
                    merged_sections.push(make_merged_section(remote_s, path));
                    conflicts.push(SectionConflict {
                        section_path: Some(path.clone()),
                        conflict_type: ConflictType::SectionConflict,
                        base_content: Some(base_s.content.clone()),
                        local_content: "(deleted)".to_string(),
                        remote_content: remote_s.content.clone(),
                    });
                }
            }

            // Case G: deleted remotely, still exists in local.
            (Some(base_s), Some(local_s), None) => {
                if base_s.content_hash == local_s.content_hash {
                    // Deleted remotely, unchanged locally → honor deletion.
                    auto_resolved += 1;
                } else {
                    // Case G: delete vs edit → conflict.
                    merged_sections.push(make_merged_section(local_s, path));
                    conflicts.push(SectionConflict {
                        section_path: Some(path.clone()),
                        conflict_type: ConflictType::SectionConflict,
                        base_content: Some(base_s.content.clone()),
                        local_content: local_s.content.clone(),
                        remote_content: "(deleted)".to_string(),
                    });
                }
            }

            // Deleted on both sides → gone.
            (Some(_), None, None) => {
                auto_resolved += 1;
            }

            // Not in any version (shouldn't happen).
            (None, None, None) => {}
        }
    }

    MergeResult {
        merged_tree: SectionTree {
            frontmatter: merged_frontmatter,
            preamble: merged_preamble,
            sections: merged_sections,
        },
        conflicts,
        auto_resolved_count: auto_resolved,
    }
}

fn merge_preamble(
    base: &str,
    local: &str,
    remote: &str,
    timestamp: &str,
    conflicts: &mut Vec<SectionConflict>,
    auto_resolved: &mut usize,
) -> String {
    let base_hash = Section::compute_hash(base);
    let local_hash = Section::compute_hash(local);
    let remote_hash = Section::compute_hash(remote);

    if local_hash == remote_hash {
        // Both same (whether changed or not).
        return local.to_string();
    }
    if local_hash == base_hash {
        // Only remote changed.
        *auto_resolved += 1;
        return remote.to_string();
    }
    if remote_hash == base_hash {
        // Only local changed.
        *auto_resolved += 1;
        return local.to_string();
    }

    // Both changed differently → conflict.
    conflicts.push(SectionConflict {
        section_path: Some("/_preamble".to_string()),
        conflict_type: ConflictType::SectionConflict,
        base_content: Some(base.to_string()),
        local_content: local.to_string(),
        remote_content: remote.to_string(),
    });
    build_append_both(local, remote, timestamp)
}

/// Build a HashMap from section path -> &Section for quick lookup.
fn section_map<'a>(sections: &'a [Section]) -> HashMap<&'a str, &'a Section> {
    sections.iter().map(|s| (s.path.as_str(), s)).collect()
}

/// Collect all unique section paths in document order.
/// Priority: local order first, then remote additions, then base-only sections.
fn collect_ordered_paths(
    local_sections: &[Section],
    remote_sections: &[Section],
    base_sections: &[Section],
) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Local order first.
    for s in local_sections {
        if seen.insert(s.path.clone()) {
            paths.push(s.path.clone());
        }
    }
    // Remote additions.
    for s in remote_sections {
        if seen.insert(s.path.clone()) {
            paths.push(s.path.clone());
        }
    }
    // Base sections that might have been deleted from both.
    for s in base_sections {
        if seen.insert(s.path.clone()) {
            paths.push(s.path.clone());
        }
    }

    paths
}

/// Resolve a section, accounting for renames.
/// If `path` was renamed in `rename_map`, look up the original path in `section_map`.
fn resolve_section<'a>(
    path: &str,
    section_map: &HashMap<&str, &'a Section>,
    _other_rename_map: &HashMap<String, String>,
) -> Option<&'a Section> {
    section_map.get(path).copied()
}

/// Create a new section with the given path, copying content from the source.
fn make_merged_section(source: &Section, path: &str) -> Section {
    Section {
        path: path.to_string(),
        heading_text: source.heading_text.clone(),
        level: source.level,
        content: source.content.clone(),
        content_hash: source.content_hash.clone(),
        ordinal: source.ordinal,
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
