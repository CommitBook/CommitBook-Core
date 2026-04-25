use std::collections::BTreeMap;

use crate::domain::conflict::ConflictType;
use crate::domain::section::Frontmatter;

use super::engine::SectionConflict;

/// Merge frontmatter from base, local, and remote.
///
/// Rules:
/// - Key changed only on one side → take changed key
/// - Different keys changed on each side → merge both keys
/// - Same key changed on both sides to different values → create conflict
/// - Key added on one side only → keep it
/// - Key deleted on one side, unchanged on other → delete it
/// - Key deleted on one side, changed on other → conflict
pub fn merge_frontmatter(
    base: &Option<Frontmatter>,
    local: &Option<Frontmatter>,
    remote: &Option<Frontmatter>,
) -> (Option<Frontmatter>, Vec<SectionConflict>) {
    let base_fields = base.as_ref().map(|f| &f.fields);
    let local_fields = local.as_ref().map(|f| &f.fields);
    let remote_fields = remote.as_ref().map(|f| &f.fields);

    // If no frontmatter exists anywhere, nothing to merge.
    if base.is_none() && local.is_none() && remote.is_none() {
        return (None, Vec::new());
    }

    // If only one side has frontmatter and it matches base, take whatever changed.
    let empty = BTreeMap::new();
    let base_f = base_fields.unwrap_or(&empty);
    let local_f = local_fields.unwrap_or(&empty);
    let remote_f = remote_fields.unwrap_or(&empty);

    // Collect all keys from all three versions.
    let mut all_keys: Vec<String> = Vec::new();
    for key in base_f.keys().chain(local_f.keys()).chain(remote_f.keys()) {
        if !all_keys.contains(key) {
            all_keys.push(key.clone());
        }
    }

    let mut merged = BTreeMap::new();
    let mut conflicts = Vec::new();

    for key in &all_keys {
        let base_val = base_f.get(key);
        let local_val = local_f.get(key);
        let remote_val = remote_f.get(key);

        match (base_val, local_val, remote_val) {
            // Unchanged on both sides.
            (Some(b), Some(l), Some(r)) if l == b && r == b => {
                merged.insert(key.clone(), b.clone());
            }
            // Changed only on local side.
            (Some(b), Some(l), Some(r)) if r == b && l != b => {
                merged.insert(key.clone(), l.clone());
            }
            // Changed only on remote side.
            (Some(b), Some(l), Some(r)) if l == b && r != b => {
                merged.insert(key.clone(), r.clone());
            }
            // Changed on both sides to the same value.
            (Some(_b), Some(l), Some(r)) if l == r => {
                merged.insert(key.clone(), l.clone());
            }
            // Changed on both sides to different values → conflict.
            (Some(_b), Some(l), Some(r)) => {
                // Keep local value in merged, but record a conflict.
                merged.insert(key.clone(), l.clone());
                conflicts.push(SectionConflict {
                    section_path: None,
                    conflict_type: ConflictType::FrontmatterConflict,
                    base_content: Some(format!("{key}: {}", _b)),
                    local_content: format!("{key}: {l}"),
                    remote_content: format!("{key}: {r}"),
                });
            }
            // Key added on local only.
            (None, Some(l), None) => {
                merged.insert(key.clone(), l.clone());
            }
            // Key added on remote only.
            (None, None, Some(r)) => {
                merged.insert(key.clone(), r.clone());
            }
            // Key added on both sides to same value.
            (None, Some(l), Some(r)) if l == r => {
                merged.insert(key.clone(), l.clone());
            }
            // Key added on both sides to different values → conflict.
            (None, Some(l), Some(r)) => {
                merged.insert(key.clone(), l.clone());
                conflicts.push(SectionConflict {
                    section_path: None,
                    conflict_type: ConflictType::FrontmatterConflict,
                    base_content: None,
                    local_content: format!("{key}: {l}"),
                    remote_content: format!("{key}: {r}"),
                });
            }
            // Key deleted on local, unchanged on remote → delete.
            (Some(b), None, Some(r)) if r == b => {
                // Deleted by local, keep deleted.
            }
            // Key deleted on remote, unchanged on local → delete.
            (Some(b), Some(l), None) if l == b => {
                // Deleted by remote, keep deleted.
            }
            // Key deleted on local, changed on remote → conflict.
            (Some(_b), None, Some(r)) => {
                merged.insert(key.clone(), r.clone());
                conflicts.push(SectionConflict {
                    section_path: None,
                    conflict_type: ConflictType::FrontmatterConflict,
                    base_content: Some(format!("{key}: {}", _b)),
                    local_content: format!("{key}: (deleted)"),
                    remote_content: format!("{key}: {r}"),
                });
            }
            // Key deleted on remote, changed on local → conflict.
            (Some(_b), Some(l), None) => {
                merged.insert(key.clone(), l.clone());
                conflicts.push(SectionConflict {
                    section_path: None,
                    conflict_type: ConflictType::FrontmatterConflict,
                    base_content: Some(format!("{key}: {}", _b)),
                    local_content: format!("{key}: {l}"),
                    remote_content: format!("{key}: (deleted)"),
                });
            }
            // Key deleted on both sides.
            (Some(_), None, None) => {
                // Both deleted, stay deleted.
            }
            // Fallback: should not occur given all keys are collected.
            _ => {}
        }
    }

    // Determine format from whichever source has frontmatter (prefer local).
    let format = local
        .as_ref()
        .or(remote.as_ref())
        .or(base.as_ref())
        .map(|f| f.format.clone());

    if merged.is_empty() && conflicts.is_empty() {
        return (None, Vec::new());
    }

    let raw = rebuild_frontmatter_raw(&merged);

    let result = format.map(|fmt| Frontmatter {
        format: fmt,
        raw,
        fields: merged,
    });

    (result, conflicts)
}

/// Rebuild the raw frontmatter string from fields.
fn rebuild_frontmatter_raw(fields: &BTreeMap<String, String>) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "frontmatter_merge_tests.rs"]
mod tests;
