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
        // Surface the original message — it's usually specific enough already
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_humanize_network() {
        let e = anyhow!("Could not resolve host: github.com").context("git fetch failed");
        let msg = humanize(&e);
        assert!(msg.contains("network"), "unexpected: {msg}");
    }

    #[test]
    fn test_humanize_non_ff() {
        let e = anyhow!("non-fast-forward").context("git push failed");
        let msg = humanize(&e);
        assert!(msg.contains("Remote has diverged"), "unexpected: {msg}");
    }

    #[test]
    fn test_humanize_auth() {
        let e = anyhow!("authentication required");
        let msg = humanize(&e);
        assert!(msg.contains("Authentication"), "unexpected: {msg}");
        assert!(msg.contains("Git credentials"), "unexpected: {msg}");
        assert!(!msg.contains("commitbook login"), "unexpected: {msg}");
    }

    #[test]
    fn test_humanize_not_initialized() {
        let e = anyhow!("CommitBook is not initialized. Run `commitbook init` first.");
        let msg = humanize(&e);
        assert!(msg.contains("commitbook init"), "unexpected: {msg}");
    }

    #[test]
    fn test_humanize_falls_back_to_top_context() {
        let e = anyhow!("low-level libgit2 error 0x42").context("Could not push origin/main");
        let msg = humanize(&e);
        // Top context wins — the libgit2 internals don't appear.
        assert!(msg.contains("Could not push"), "unexpected: {msg}");
        assert!(!msg.contains("libgit2"), "leaked internals: {msg}");
    }
}
