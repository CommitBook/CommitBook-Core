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
pub fn generate_timestamp_message(_summary: &ChangesSummary) -> String {
    format!("Writing {}", datetime::now_formatted())
}

#[cfg(test)]
#[path = "fallback_tests.rs"]
mod tests;
