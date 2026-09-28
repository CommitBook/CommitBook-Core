use anyhow::{bail, Result};
use colored::Colorize;
use std::io::{self, BufRead, IsTerminal};
use std::path::Path;

use commitbook_engine::config::LocalConfig;
use commitbook_engine::state::auth::{AuthConfig, AuthEntry};

pub async fn set(cb_dir: &Path, repo_root: &Path, provider: Option<String>) -> Result<()> {
    let token = if io::stdin().is_terminal() {
        // Typed input is not echoed, so the token never shows on screen.
        parse_token(&rpassword::prompt_password(
            "  Enter personal access token: ",
        )?)?
    } else {
        read_token(&mut io::stdin().lock())?
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
        "  {} Token stored for the {} provider (not checked against it).",
        "OK".green().bold(),
        provider.cyan()
    );
    println!(
        "  {}",
        "Token stored in .CommitBook/local/auth.toml (gitignored).".dimmed()
    );
    println!(
        "  {}",
        "Desktop git sync uses your normal Git credentials; this token is only for token-backed transports.".dimmed()
    );

    Ok(())
}

pub fn clear(cb_dir: &Path) -> Result<()> {
    if AuthConfig::clear(cb_dir)? {
        println!(
            "  {} Removed token from .CommitBook/local/auth.toml.",
            "OK".green().bold()
        );
    } else {
        println!("  {}", "No token stored.".dimmed());
    }
    Ok(())
}

/// Read the token from the first line of piped input.
fn read_token(input: &mut impl BufRead) -> Result<String> {
    let mut line = String::new();
    input.read_line(&mut line)?;
    parse_token(&line)
}

/// The token from one line of input, without surrounding whitespace.
fn parse_token(input: &str) -> Result<String> {
    let token = input.trim();
    if token.is_empty() {
        bail!("Token cannot be empty.");
    }
    if token.chars().any(char::is_whitespace) {
        bail!("Token must be a single line without spaces.");
    }
    Ok(token.to_string())
}

/// Detect git provider from the configured remote's URL.
fn detect_provider(repo_root: &Path) -> String {
    let remote = LocalConfig::load_read_only(repo_root)
        .map(|c| c.git.remote)
        .unwrap_or_else(|_| "origin".to_string());
    if let Ok(output) = std::process::Command::new("git")
        .args(["remote", "get-url", &remote])
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
#[path = "token_tests.rs"]
mod tests;
