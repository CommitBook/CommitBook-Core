use anyhow::Result;
use std::path::Path;
pub fn run(root: &Path, json: bool) -> Result<()> {
    let preview = commitbook_engine::inspection::preview(root);
    if json {
        println!("{}", serde_json::to_string_pretty(&preview)?);
    } else {
        println!("{}", preview.policy);
        println!(
            "Repository: {}\nBranch: {}\nRemote: {}\nAuto push: {}",
            preview.repository,
            preview.branch.as_deref().unwrap_or("unknown"),
            preview.remote.as_deref().unwrap_or("unknown"),
            preview
                .auto_push
                .map(|v| if v { "yes" } else { "no" })
                .unwrap_or("unknown")
        );
        for entry in &preview.entries {
            println!(
                "  {} {} (staged: {}, unstaged: {})",
                entry.change, entry.path, entry.staged, entry.unstaged
            );
        }
        for blocker in &preview.blockers {
            println!("Blocked: {blocker}");
        }
    }
    anyhow::ensure!(
        preview.blockers.is_empty(),
        "Preview found blockers; resolve them before syncing"
    );
    Ok(())
}
