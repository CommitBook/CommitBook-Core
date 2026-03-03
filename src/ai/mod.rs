pub mod copilot;
pub mod claude;
pub mod codex;
pub mod fallback;

use crate::git::operations::ChangesSummary;

/// Generate a commit message using the configured provider chain.
/// Tries each provider in order: gh-copilot -> claude-cli -> codex-cli -> fallback.
pub async fn generate_commit_message(
    summary: &ChangesSummary,
    providers: &[String],
    repo_path: &std::path::Path,
) -> String {
    for provider in providers {
        let result = match provider.as_str() {
            "gh-copilot" => copilot::generate(summary, repo_path).await,
            "claude-cli" => claude::generate(summary, repo_path).await,
            "codex-cli" => codex::generate(summary, repo_path).await,
            "fallback" => Ok(fallback::generate(summary)),
            _ => continue,
        };

        match result {
            Ok(msg) if !msg.trim().is_empty() => return msg,
            Ok(_) => continue,
            Err(e) => {
                log::warn!("Provider '{}' failed: {}", provider, e);
                continue;
            }
        }
    }

    // Ultimate fallback
    fallback::generate(summary)
}

/// Check if a specific AI provider is available on the system.
pub fn is_provider_available(provider: &str) -> bool {
    match provider {
        "gh-copilot" => copilot::is_available(),
        "claude-cli" => claude::is_available(),
        "codex-cli" => codex::is_available(),
        "fallback" => true,
        _ => false,
    }
}
