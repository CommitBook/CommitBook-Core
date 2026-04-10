use anyhow::{bail, Result};
use colored::Colorize;
use std::io::{self, Write};
use std::path::Path;

use commitbook_core::state::auth::{AuthConfig, AuthEntry};

pub async fn run(
    cb_dir: &Path,
    repo_root: &Path,
    token: Option<String>,
    provider: Option<String>,
) -> Result<()> {
    let token = match token {
        Some(t) => t,
        None => {
            // Prompt for token interactively.
            print!("  Enter personal access token: ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let t = input.trim().to_string();
            if t.is_empty() {
                bail!("Token cannot be empty.");
            }
            t
        }
    };

    // Auto-detect provider from remote URL if not specified.
    let provider = match provider {
        Some(p) => p,
        None => detect_provider(repo_root),
    };

    let auth = AuthConfig {
        auth: AuthEntry {
            provider: Some(provider.clone()),
            token: Some(token),
        },
    };

    auth.save(cb_dir)?;

    println!(
        "  {} Authenticated with {} provider.",
        "OK".green().bold(),
        provider.cyan()
    );
    println!(
        "  {}",
        "Token stored in .CommitBook/auth.toml (gitignored).".dimmed()
    );

    Ok(())
}

/// Detect git provider from remote URL.
fn detect_provider(repo_root: &Path) -> String {
    // Try to detect from git remote URL.
    if let Ok(output) = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout);
            if url.contains("github.com") {
                return "github".to_string();
            }
            if url.contains("gitlab.com") || url.contains("gitlab") {
                return "gitlab".to_string();
            }
            if url.contains("codeberg.org") {
                return "codeberg".to_string();
            }
        }
    }
    "generic".to_string()
}

#[cfg(test)]
#[path = "login_tests.rs"]
mod tests;
