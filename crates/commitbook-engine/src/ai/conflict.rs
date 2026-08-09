//! Conflict resolvers receive the three structured index sides and return an
//! explicit content-or-deletion resolution.
//!
//! Mirrors the `CommitMessageProvider` pattern in `super::mod`: each
//! resolver wraps a CLI tool, the registry dispatches by config key, and
//! `manual` is its own pseudo-key signaling "leave markers in place".

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

use crate::git::GitConflict;

#[cfg(not(any(target_os = "ios", target_os = "android")))]
use super::{claude, codex, copilot, cursor, gemini};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolution {
    WriteContent(String),
    DeleteFile,
}

/// Trait for AI-powered git-conflict resolution.
#[async_trait]
pub trait ConflictResolver: Send + Sync {
    /// Human-readable resolver name (e.g. "Claude Code").
    fn name(&self) -> &str;

    /// Registry key used in config (e.g. "claude").
    fn key(&self) -> &str;

    /// Check if this resolver's CLI tool is installed and reachable.
    fn is_available(&self) -> bool;

    /// Resolve one structured index conflict. Implementations intended for
    /// text models should reject binary, symlink, and gitlink conflicts.
    async fn resolve(&self, conflict: &GitConflict, repo_path: &Path)
        -> Result<ConflictResolution>;
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
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn strip_outer_code_fence(s: &str) -> String {
    let trimmed = s.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }
    let after_open = match trimmed.find('\n') {
        Some(idx) => &trimmed[idx + 1..],
        None => return trimmed.to_string(),
    };
    // Only unwrap when the closing fence is the final content: otherwise a note
    // that legitimately opens with a fenced block followed by prose (e.g.
    // "```py\ncode\n```\nprose") would be truncated at the inner fence.
    let inner = match after_open.rfind("```") {
        Some(idx) if after_open[idx + 3..].trim().is_empty() => {
            after_open[..idx].trim_end_matches('\n')
        }
        _ => return trimmed.to_string(),
    };
    inner.to_string()
}

/// Build the prompt sent to a resolver CLI for a single structured text
/// conflict. Deleted sides are represented explicitly; no working-tree marker
/// parsing is involved.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub(crate) fn build_resolve_prompt(conflict: &GitConflict) -> Result<String> {
    if conflict.is_binary_or_special() {
        anyhow::bail!(
            "Conflict {} is binary or special and cannot be resolved as text",
            conflict.path
        );
    }
    let ancestor = conflict.ancestor_text().unwrap_or("<deleted>");
    let local = conflict.local_text().unwrap_or("<deleted>");
    let remote = conflict.remote_text().unwrap_or("<deleted>");
    Ok(format!(
        "Resolve the three git index versions below into the final file. Output ONLY the \
         resolved file contents with no commentary, no markdown fencing, and no conflict \
         markers. Preserve non-conflicting content. If one side is <deleted>, decide whether \
         the final file should remain based on the other versions; this text-only CLI cannot \
         request deletion, so fail rather than returning an explanation when deletion is the \
         only correct result.\n\nFile: {}\n\nANCESTOR:\n{}\n\nLOCAL:\n{}\n\nREMOTE:\n{}",
        conflict.path, ancestor, local, remote
    ))
}

#[cfg(test)]
#[path = "conflict_tests.rs"]
mod tests;
