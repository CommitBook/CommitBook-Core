use crate::domain::section::Frontmatter;

use super::engine::SectionConflict;

/// Merge frontmatter from base, local, and remote.
///
/// Rules:
/// - Key changed only on one side → take changed key
/// - Different keys changed on each side → merge keys
/// - Same key changed on both sides → create conflict
pub fn merge_frontmatter(
    _base: &Option<Frontmatter>,
    _local: &Option<Frontmatter>,
    _remote: &Option<Frontmatter>,
) -> (Option<Frontmatter>, Vec<SectionConflict>) {
    // TODO: Implement in Phase 1
    todo!("frontmatter merge will be implemented in Phase 1")
}
