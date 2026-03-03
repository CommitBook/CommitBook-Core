use anyhow::{bail, Result};
use std::path::Path;

use crate::git::operations::ChangesSummary;

/// Check if Claude Code CLI is available.
pub fn is_available() -> bool {
    which::which("claude").is_ok()
}

/// Generate a commit message using Claude Code CLI.
/// This is a placeholder for future implementation.
pub async fn generate(_summary: &ChangesSummary, _repo_path: &Path) -> Result<String> {
    if !is_available() {
        bail!("Claude Code CLI is not available");
    }

    // TODO: Implement Claude Code CLI integration when available
    bail!("Claude Code CLI commit message generation not yet implemented")
}
