use anyhow::{bail, Result};
use colored::Colorize;

/// Authenticate with a personal access token.
pub async fn pat() -> Result<()> {
    println!("{}", "PAT Authentication".bold());
    println!();
    println!("Create a personal access token at:");
    println!("  https://github.com/settings/tokens/new");
    println!();
    println!("Required scopes: repo");
    println!();

    // Read token from stdin.
    println!("Paste your token:");
    let mut token = String::new();
    std::io::stdin().read_line(&mut token)?;
    let token = token.trim().to_string();

    if token.is_empty() {
        bail!("No token provided.");
    }

    // Validate the token by calling GitHub API.
    let client = commitbook_core::transport::github_api::github_client(&token)?;
    match commitbook_core::transport::github_api::validate_token(&client).await {
        Ok(()) => {
            println!("{}", "Token validated successfully.".green().bold());
            println!();
            println!("To create a workspace with this token:");
            println!("  commitbook workspace add --pat --owner <owner> --repo <repo>");
            println!();
            println!(
                "{}",
                "Note: Token storage will be implemented with Keychain integration."
                    .dimmed()
            );
        }
        Err(e) => {
            bail!("Token validation failed: {e}");
        }
    }

    Ok(())
}

/// Authenticate with GitHub via the CommitBook backend.
pub async fn github() -> Result<()> {
    println!("{}", "GitHub Login".bold());
    println!();
    println!(
        "{}",
        "GitHub App login requires the CommitBook backend to be running.".yellow()
    );
    println!("This feature will be available when the backend is deployed.");
    println!();
    println!("Alternative: use `commitbook login pat` for now.");

    Ok(())
}
