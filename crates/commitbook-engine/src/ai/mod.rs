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

/// Wait for a child process with a timeout, draining stdout and stderr on
/// separate threads so a child that fills the pipe buffer cannot deadlock the
/// parent (the previous per-provider version read the pipes only after the
/// child exited, which hung on output larger than the ~64 KB pipe buffer).
///
/// All resolver/provider callers run inside `spawn_blocking`, so the blocking
/// reader threads are fine.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    use std::io::Read;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("Process timed out after {:?}", timeout);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => anyhow::bail!("Error waiting for process: {}", e),
        }
    };

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
