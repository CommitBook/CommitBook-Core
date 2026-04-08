use crate::domain::conflict::ConflictType;
use crate::domain::section::SectionTree;

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
    _base: &SectionTree,
    _local: &SectionTree,
    _remote: &SectionTree,
) -> MergeResult {
    // TODO: Implement in Phase 1
    todo!("merge_document will be implemented in Phase 1")
}
