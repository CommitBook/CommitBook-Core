use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use super::CommitMessageProvider;
use crate::git::ChangesSummary;
use crate::utils::datetime;

pub struct FallbackProvider;

#[async_trait]
impl CommitMessageProvider for FallbackProvider {
    fn name(&self) -> &str {
        "Fallback"
    }

    fn key(&self) -> &str {
        "fallback"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn generate(&self, summary: &ChangesSummary, _repo_path: &Path) -> Result<String> {
        Ok(generate_timestamp_message(summary))
    }
}

/// Generate a simple timestamp-based commit message.
pub fn generate_timestamp_message(summary: &ChangesSummary) -> String {
    let timestamp = datetime::now_formatted();

    if summary.is_empty() {
        return format!("Writing {}", timestamp);
    }

    format!("Writing {} ({})", timestamp, summary.to_summary_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_no_changes() {
        let summary = ChangesSummary::default();
        let msg = generate_timestamp_message(&summary);
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
        let msg = generate_timestamp_message(&summary);
        assert!(msg.starts_with("Writing "));
        assert!(msg.contains("1 new"));
        assert!(msg.contains("1 modified"));
    }
}
