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
#[path = "mod_tests.rs"]
mod tests;
