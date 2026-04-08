pub mod claude;
pub mod codex;
pub mod copilot;
pub mod fallback;

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use crate::git::ChangesSummary;

/// Trait for AI-powered commit message generation.
#[async_trait]
pub trait CommitMessageProvider: Send + Sync {
    /// Human-readable provider name (e.g. "Claude Code").
    fn name(&self) -> &str;

    /// Registry key used in config (e.g. "claude-cli").
    fn key(&self) -> &str;

    /// Check if this provider's CLI tool is installed and reachable.
    fn is_available(&self) -> bool;

    /// Generate a commit message from a changes summary.
    async fn generate(&self, summary: &ChangesSummary, repo_path: &Path) -> Result<String>;
}

/// Ordered chain of providers — tries each in sequence, falls back to timestamp.
pub struct ProviderChain {
    providers: Vec<Box<dyn CommitMessageProvider>>,
}

impl ProviderChain {
    /// Build the default provider chain.
    pub fn new() -> Self {
        Self {
            providers: vec![
                Box::new(copilot::CopilotProvider),
                Box::new(claude::ClaudeProvider),
                Box::new(codex::CodexProvider),
                Box::new(fallback::FallbackProvider),
            ],
        }
    }

    /// Try each provider in the given order. Returns (message, provider_name).
    pub async fn generate(
        &self,
        summary: &ChangesSummary,
        provider_keys: &[String],
        repo_path: &Path,
    ) -> (String, String) {
        for key in provider_keys {
            if let Some(provider) = self.providers.iter().find(|p| p.key() == key.as_str()) {
                if !provider.is_available() {
                    continue;
                }
                match provider.generate(summary, repo_path).await {
                    Ok(msg) if !msg.trim().is_empty() => {
                        return (msg, provider.name().to_string());
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        log::warn!("Provider '{}' failed: {}", provider.name(), e);
                        continue;
                    }
                }
            }
        }

        // Ultimate fallback — always succeeds
        let fb = fallback::FallbackProvider;
        let msg = fb.generate(summary, repo_path).await.unwrap();
        (msg, fb.name().to_string())
    }

    /// Return availability status for each requested provider key.
    pub fn check_availability(&self, provider_keys: &[String]) -> Vec<(String, String, bool)> {
        provider_keys
            .iter()
            .map(|key| {
                if let Some(p) = self.providers.iter().find(|p| p.key() == key.as_str()) {
                    (key.clone(), p.name().to_string(), p.is_available())
                } else {
                    (key.clone(), key.clone(), false)
                }
            })
            .collect()
    }
}

impl Default for ProviderChain {
    fn default() -> Self {
        Self::new()
    }
}

/// Truncate a string to max_len, appending "..." if truncated.
pub(crate) fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Clean up AI-generated commit message text.
pub(crate) fn clean_message(raw: &str) -> String {
    let mut msg = raw.trim().to_string();

    // Strip markdown code fences
    msg = msg.trim_start_matches("```").trim_end_matches("```").to_string();
    // Strip surrounding quotes
    msg = msg.trim_matches('"').trim_matches('\'').trim_matches('`').to_string();
    msg = msg.trim().to_string();

    // Take only the first line
    if let Some(idx) = msg.find('\n') {
        msg.truncate(idx);
    }

    // Truncate to 72 chars
    if msg.len() > 72 {
        msg.truncate(72);
    }

    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_message_passthrough() {
        assert_eq!(clean_message("Fix login bug"), "Fix login bug");
    }

    #[test]
    fn test_clean_message_strips_fences() {
        assert_eq!(clean_message("```Update README```"), "Update README");
    }

    #[test]
    fn test_clean_message_strips_quotes() {
        assert_eq!(clean_message("\"Add tests\""), "Add tests");
        assert_eq!(clean_message("'Add tests'"), "Add tests");
        assert_eq!(clean_message("`Add tests`"), "Add tests");
    }

    #[test]
    fn test_clean_message_first_line_only() {
        assert_eq!(
            clean_message("First line\nSecond line\nThird line"),
            "First line"
        );
    }

    #[test]
    fn test_clean_message_truncates_to_72() {
        let long = "A".repeat(100);
        let result = clean_message(&long);
        assert_eq!(result.len(), 72);
    }

    #[test]
    fn test_truncate_short_passthrough() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_appends_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hello...");
    }

    // --- MockProvider + ProviderChain async tests ---

    struct MockProvider {
        name: &'static str,
        key: &'static str,
        available: bool,
        response: Option<&'static str>,
    }

    #[async_trait]
    impl CommitMessageProvider for MockProvider {
        fn name(&self) -> &str { self.name }
        fn key(&self) -> &str { self.key }
        fn is_available(&self) -> bool { self.available }

        async fn generate(&self, _summary: &ChangesSummary, _repo_path: &Path) -> Result<String> {
            match self.response {
                Some(msg) => Ok(msg.to_string()),
                None => anyhow::bail!("mock error"),
            }
        }
    }

    #[tokio::test]
    async fn test_chain_skips_unavailable() {
        let chain = ProviderChain {
            providers: vec![
                Box::new(MockProvider { name: "Unavail", key: "unavail", available: false, response: Some("nope") }),
                Box::new(MockProvider { name: "Avail", key: "avail", available: true, response: Some("good msg") }),
            ],
        };
        let summary = ChangesSummary { new_files: vec!["a.txt".into()], ..Default::default() };
        let (msg, provider) = chain.generate(&summary, &["unavail".into(), "avail".into()], Path::new("/tmp")).await;
        assert_eq!(msg, "good msg");
        assert_eq!(provider, "Avail");
    }

    #[tokio::test]
    async fn test_chain_all_fail_uses_fallback() {
        let chain = ProviderChain {
            providers: vec![
                Box::new(MockProvider { name: "Bad", key: "bad", available: true, response: None }),
                Box::new(fallback::FallbackProvider),
            ],
        };
        let summary = ChangesSummary { new_files: vec!["a.txt".into()], ..Default::default() };
        let (msg, provider) = chain.generate(&summary, &["bad".into()], Path::new("/tmp")).await;
        assert_eq!(provider, "Fallback");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_check_availability() {
        let chain = ProviderChain {
            providers: vec![
                Box::new(MockProvider { name: "Yes", key: "yes", available: true, response: None }),
                Box::new(MockProvider { name: "No", key: "no", available: false, response: None }),
            ],
        };
        let result = chain.check_availability(&["yes".into(), "no".into(), "missing".into()]);
        assert_eq!(result.len(), 3);
        assert!(result[0].2);          // "yes" is available
        assert!(!result[1].2);         // "no" is not
        assert!(!result[2].2);         // "missing" is not
    }
}
