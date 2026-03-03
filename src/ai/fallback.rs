use crate::git::operations::ChangesSummary;
use crate::utils::datetime;

/// Generate a simple timestamp-based commit message as fallback.
pub fn generate(summary: &ChangesSummary) -> String {
    let timestamp = datetime::now_formatted();
    let total = summary.total();

    if total == 0 {
        return format!("Writing {}", timestamp);
    }

    let mut parts = Vec::new();
    if !summary.new_files.is_empty() {
        parts.push(format!("{} new", summary.new_files.len()));
    }
    if !summary.modified_files.is_empty() {
        parts.push(format!("{} modified", summary.modified_files.len()));
    }
    if !summary.deleted_files.is_empty() {
        parts.push(format!("{} deleted", summary.deleted_files.len()));
    }

    format!("Writing {} ({})", timestamp, parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_no_changes() {
        let summary = ChangesSummary::default();
        let msg = generate(&summary);
        assert!(msg.starts_with("Writing "));
        assert!(!msg.contains('('));
    }

    #[test]
    fn test_fallback_with_changes() {
        let summary = ChangesSummary {
            new_files: vec!["test.md".to_string()],
            modified_files: vec!["notes.md".to_string()],
            deleted_files: vec![],
        };
        let msg = generate(&summary);
        assert!(msg.starts_with("Writing "));
        assert!(msg.contains("1 new"));
        assert!(msg.contains("1 modified"));
    }
}
