#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod claude;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod codex;
pub mod conflict;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod copilot;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod cursor;
pub mod fallback;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod gemini;

pub use conflict::{ConflictResolution, ConflictResolver, ResolverRegistry};

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use crate::config::values::ANY_AGENT_ORDER;
use crate::config::{Agent, CommitAgent, CommitMode};
use crate::git::ChangesSummary;

/// Provider keys to try, in order, for a commit message.
///
/// `timestamp` skips the AI CLIs entirely and uses only the deterministic
/// `fallback` provider. `ai` tries the configured agent (or, for `any`,
/// every agent in `ANY_AGENT_ORDER`), then the fallback.
pub fn commit_provider_keys(mode: CommitMode, agent: CommitAgent) -> Vec<String> {
    let agents: Vec<Agent> = match (mode, agent.agent()) {
        (CommitMode::Timestamp, _) => Vec::new(),
        (CommitMode::Ai, Some(agent)) => vec![agent],
        (CommitMode::Ai, None) => ANY_AGENT_ORDER.to_vec(),
    };
    agents
        .into_iter()
        .map(|agent| agent.commit_provider_key().to_string())
        .chain(std::iter::once("fallback".to_string()))
        .collect()
}

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

/// Ordered chain of providers, tries each in sequence, falls back to timestamp.
pub struct ProviderChain {
    providers: Vec<Box<dyn CommitMessageProvider>>,
}

impl ProviderChain {
    /// Build the default provider chain.
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut providers: Vec<Box<dyn CommitMessageProvider>> = Vec::new();
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            providers.push(Box::new(copilot::CopilotProvider));
            providers.push(Box::new(claude::ClaudeProvider));
            providers.push(Box::new(codex::CodexProvider));
            providers.push(Box::new(gemini::GeminiProvider));
            providers.push(Box::new(cursor::CursorProvider));
        }
        providers.push(Box::new(fallback::FallbackProvider));
        Self { providers }
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

        // Ultimate fallback, always succeeds
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

/// Prompt asking an AI CLI for a one-line commit message.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn commit_message_prompt(summary: &ChangesSummary, repo_path: &Path) -> String {
    let diff_summary = crate::git::GitRepo::open(repo_path)
        .and_then(|r| r.diff_summary())
        .unwrap_or_default();
    format!(
        "Write a single-line git commit message in imperative mood (max 72 chars, no quotes, no markdown, no prefix) describing what changed. Do not describe the diff itself or mention 'staged'/'unstaged'. Files: {}. Changes:\n{}",
        summary.to_summary_text(),
        truncate(&diff_summary, 500)
    )
}

/// Clean an AI CLI's commit-message output, rejecting empty replies and
/// narration of the diff so the chain falls through to the next provider.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn finish_commit_message(output: &std::process::Output, cli: &str) -> Result<String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{cli} failed: {}", stderr.trim());
    }
    let msg = clean_message(&String::from_utf8_lossy(&output.stdout));
    if msg.is_empty() {
        anyhow::bail!("Empty response from {cli}");
    }
    if looks_like_diff_narration(&msg) {
        anyhow::bail!("{cli} returned diff narration, not a commit message");
    }
    Ok(msg)
}

/// Truncate a string to max_len, appending "..." if truncated.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Returns true if the message reads like the AI is describing the diff
/// itself rather than the change. Triggers a fall-through to the next provider.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn looks_like_diff_narration(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    if lower.contains("staged") && lower.contains("unstaged") {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "the diff ",
        "the staged ",
        "the unstaged ",
        "the changes show ",
        "this diff ",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Clean up AI-generated commit message text.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn clean_message(raw: &str) -> String {
    let mut msg = raw.trim().to_string();

    // Strip markdown code fences
    msg = msg
        .trim_start_matches("```")
        .trim_end_matches("```")
        .to_string();
    // Strip surrounding quotes
    msg = msg
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .to_string();
    msg = msg.trim().to_string();

    // Take only the first line
    if let Some(idx) = msg.find('\n') {
        let mut end = idx;
        while end > 0 && !msg.is_char_boundary(end) {
            end -= 1;
        }
        msg.truncate(end);
    }

    // Truncate to 72 chars
    if msg.len() > 72 {
        let mut end = 72;
        while end > 0 && !msg.is_char_boundary(end) {
            end -= 1;
        }
        msg.truncate(end);
    }

    msg
}

/// Run a CLI with `prompt` on stdin, bounded by `timeout`. Blocks the
/// calling thread; async callers wrap it in `spawn_blocking`.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn run_with_prompt(
    command: &mut std::process::Command,
    prompt: &str,
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    crate::process::run_bounded(command, Some(prompt.as_bytes()), timeout)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
