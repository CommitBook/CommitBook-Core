use anyhow::{bail, Context, Result};
use colored::Colorize;

use commitbook_core::domain::workspace::WorkspaceMode;
use commitbook_core::storage::{db, workspace_repo};
use commitbook_core::sync::scheduler::sync_workspace;
use commitbook_core::transport::local_repo::LocalRepoTransport;

use super::init;

pub async fn run(workspace_id: Option<&str>) -> Result<()> {
    let db_path = init::db_path()?;
    if !db_path.exists() {
        bail!("CommitBook not initialized. Run `commitbook init` first.");
    }
    let conn = db::open_database(&db_path)?;

    let workspaces = if let Some(id) = workspace_id {
        let ws = workspace_repo::get(&conn, id)?
            .with_context(|| format!("Workspace '{id}' not found"))?;
        vec![ws]
    } else {
        let all = workspace_repo::list(&conn)?;
        if all.is_empty() {
            println!("No workspaces configured.");
            return Ok(());
        }
        all
    };

    for ws in &workspaces {
        println!(
            "{} {} ({})",
            "Syncing".cyan().bold(),
            ws.name,
            ws.id.dimmed()
        );

        let transport = create_transport(ws)?;
        match sync_workspace(ws, transport.as_ref(), &conn).await {
            Ok(result) => {
                if result.pulled > 0 {
                    println!("  {} {} files", "Pulled".green(), result.pulled);
                }
                if result.pushed > 0 {
                    println!("  {} {} files", "Pushed".green(), result.pushed);
                }
                if result.conflicts > 0 {
                    println!(
                        "  {} {} conflicts",
                        "Conflicts".red().bold(),
                        result.conflicts
                    );
                }
                if result.pulled == 0 && result.pushed == 0 && result.conflicts == 0 {
                    println!("  {}", "Already up to date.".dimmed());
                }
                for err in &result.errors {
                    println!("  {} {err}", "Error:".red());
                }
            }
            Err(e) => {
                println!("  {} {e}", "Failed:".red().bold());
            }
        }
    }

    Ok(())
}

fn create_transport(
    ws: &commitbook_core::domain::workspace::Workspace,
) -> Result<Box<dyn commitbook_core::domain::transport::RemoteTransport>> {
    match ws.mode {
        WorkspaceMode::ExistingLocalRepo => {
            Ok(Box::new(LocalRepoTransport::new(
                std::path::PathBuf::from(&ws.local_root),
                ws.branch.clone(),
            )))
        }
        WorkspaceMode::Ssh => {
            let remote_url = ws
                .remote_url
                .as_ref()
                .context("SSH workspace missing remote URL")?;
            Ok(Box::new(
                commitbook_core::transport::ssh_git::SshGitTransport::new(
                    std::path::PathBuf::from(&ws.local_root),
                    remote_url.clone(),
                    ws.branch.clone(),
                ),
            ))
        }
        WorkspaceMode::Pat => {
            bail!("PAT transport requires authentication. Run `commitbook login pat` first.");
        }
        WorkspaceMode::GithubApp => {
            bail!("GitHub App transport requires backend session. Run `commitbook login github` first.");
        }
    }
}
