use anyhow::{bail, Result};
use std::path::Path;

use crate::git::operations::ChangesSummary;

/// Check if Codex CLI is available.
pub fn is_available() -> bool {
    which::which("codex").is_ok()
}

/// Generate a commit message using Codex CLI.
/// This is a placeholder for future implementation.
pub async fn generate(_summary: &ChangesSummary, _repo_path: &Path) -> Result<String> {
    if !is_available() {
        bail!("Codex CLI is not available");
    }

    // TODO: Implement Codex CLI integration when available
    bail!("Codex CLI commit message generation not yet implemented")
}
