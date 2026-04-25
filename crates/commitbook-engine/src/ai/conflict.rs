//! Conflict resolvers — invoke an AI CLI to rewrite a file containing git
//! conflict markers and return resolved content.
//!
//! Mirrors the `CommitMessageProvider` pattern in `super::mod`: each
//! resolver wraps a CLI tool, the registry dispatches by config key, and
//! `manual` is its own pseudo-key signaling "leave markers in place".

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

#[cfg(not(any(target_os = "ios", target_os = "android")))]
use super::{claude, codex, copilot, cursor, gemini};

/// Trait for AI-powered git-conflict resolution.
#[async_trait]
pub trait ConflictResolver: Send + Sync {
    /// Human-readable resolver name (e.g. "Claude Code").
    fn name(&self) -> &str;

    /// Registry key used in config (e.g. "claude").
    fn key(&self) -> &str;

    /// Check if this resolver's CLI tool is installed and reachable.
    fn is_available(&self) -> bool;

    /// Resolve a single file containing `<<<<<<<` / `=======` / `>>>>>>>`
    /// markers. Returns the file contents with all markers removed.
    async fn resolve(
        &self,
        file_path: &Path,
        content_with_markers: &str,
        repo_path: &Path,
    ) -> Result<String>;
}

/// Registry of conflict resolvers keyed by config string.
pub struct ResolverRegistry {
    resolvers: Vec<Box<dyn ConflictResolver>>,
}

impl ResolverRegistry {
    /// Build the default resolver registry.
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut resolvers: Vec<Box<dyn ConflictResolver>> = Vec::new();
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            resolvers.push(Box::new(claude::ClaudeProvider));
            resolvers.push(Box::new(codex::CodexProvider));
            resolvers.push(Box::new(copilot::CopilotProvider));
            resolvers.push(Box::new(gemini::GeminiProvider));
            resolvers.push(Box::new(cursor::CursorProvider));
        }
        Self { resolvers }
    }

    /// Look up a resolver by config key. Returns `None` for unknown keys,
    /// `manual`, or resolvers whose CLI is not installed.
    pub fn get(&self, key: &str) -> Option<&dyn ConflictResolver> {
        if key == "manual" {
            return None;
        }
        self.resolvers
            .iter()
            .find(|r| r.key() == key && r.is_available())
            .map(|r| r.as_ref())
    }

    /// Return availability status for each resolver key. Used by
    /// `commitbook doctor` to report which CLIs are installed.
    pub fn check_availability(&self) -> Vec<(String, String, bool)> {
        self.resolvers
            .iter()
            .map(|r| (r.key().to_string(), r.name().to_string(), r.is_available()))
            .collect()
    }
}

impl Default for ResolverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Strip code-fence framing that AI CLIs sometimes add around file content.
///
/// AI CLIs frequently wrap multi-line responses in ` ``` ` fences (with or
/// without a language tag). When we ask one to return the resolved file
/// content, we want the inner content, not the fence. Strips at most one
/// outer fence pair; preserves nested fences.
pub(crate) fn strip_outer_code_fence(s: &str) -> String {
    let trimmed = s.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let after_open = match trimmed.find('\n') {
        Some(idx) => &trimmed[idx + 1..],
        None => return trimmed.to_string(),
    };
    let inner = match after_open.rfind("```") {
        Some(idx) => after_open[..idx].trim_end_matches('\n'),
        None => return trimmed.to_string(),
    };
    inner.to_string()
}

/// Build the prompt sent to a resolver CLI for a single conflicted file.
pub(crate) fn build_resolve_prompt(file_path: &Path, content_with_markers: &str) -> String {
    let display = file_path.display();
    format!(
        "Resolve all git merge conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`) \
         in the file below. Output ONLY the resolved file contents with no \
         markers, no commentary, no markdown fencing — exactly what should be \
         written back to disk. Preserve all non-conflicting content verbatim.\n\
         \n\
         File: {display}\n\
         \n\
         {content_with_markers}"
    )
}

#[cfg(test)]
#[path = "conflict_tests.rs"]
mod tests;
