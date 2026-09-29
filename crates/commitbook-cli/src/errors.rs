//! Error humanization for the CLI top-level handler.
//!
//! Rust/anyhow chains expose internal context lines that read fine for
//! engineers debugging but mean little to a user who just wants to know
//! what went wrong. This module pattern-matches on common failures and
//! emits a single human-readable line; `--verbose` bypasses the matcher
//! and prints the full chain.

/// Map a top-level error to a one-line user-facing message.
pub fn humanize(e: &anyhow::Error) -> String {
    let chain: Vec<String> = e.chain().map(|c| c.to_string()).collect();
    let joined = chain.join(" / ").to_lowercase();

    if joined.contains("commitbook is not initialized") {
        return "CommitBook is not initialized in this directory. Run `commitbook init` first."
            .to_string();
    }
    if joined.contains("not a git repository") {
        return "This directory is not a git repository. Run `git init` first.".to_string();
    }
    if joined.contains("could not resolve host")
        || joined.contains("connection refused")
        || joined.contains("network is unreachable")
        || joined.contains("connection timed out")
    {
        return "Cannot reach the remote. Check your network connection and remote URL."
            .to_string();
    }
    if joined.contains("non-fast-forward") || joined.contains("rejected") {
        return "Remote has diverged. CommitBook will reconcile on the next sync.".to_string();
    }
    if joined.contains("authentication") || joined.contains("auth required") {
        return "Authentication failed. Check your Git credentials, credential helper, or SSH agent."
            .to_string();
    }
    if joined.contains("no such file") || joined.contains("not found") {
        // Surface the original message, it's usually specific enough already
        // (e.g. "Configured remote `upstream` not found").
        return chain
            .first()
            .cloned()
            .unwrap_or_else(|| "File not found.".into());
    }

    // Fallback: top-level context wins; it usually carries the most user-
    // relevant phrasing thanks to `.with_context` lines in the call sites.
    chain
        .first()
        .cloned()
        .unwrap_or_else(|| "Unknown error.".into())
}

/// True when `--verbose` would print more than `humanize` does, so the
/// "Run with --verbose" hint is only shown when it helps.
pub fn verbose_adds_detail(e: &anyhow::Error) -> bool {
    format!("{e:#}") != humanize(e)
}

/// An error the command already printed. `entry` exits non-zero without
/// printing it a second time; `Display` still shows the full chain.
#[derive(Debug)]
pub struct Reported(pub anyhow::Error);

impl std::fmt::Display for Reported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl std::error::Error for Reported {}

#[cfg(test)]
#[path = "errors_tests.rs"]
mod tests;
