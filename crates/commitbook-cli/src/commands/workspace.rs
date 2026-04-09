use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::PathBuf;

use commitbook_core::domain::workspace::*;
use commitbook_core::storage::{db, workspace_repo};

use super::init;

fn open_db() -> Result<rusqlite::Connection> {
    let path = init::db_path()?;
    if !path.exists() {
        bail!("CommitBook not initialized. Run `commitbook init` first.");
    }
    db::open_database(&path)
}

pub fn list(json: bool) -> Result<()> {
    let conn = open_db()?;
    let workspaces = workspace_repo::list(&conn)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&workspaces)?);
        return Ok(());
    }

    if workspaces.is_empty() {
        println!("No workspaces configured.");
        println!("  Add one: commitbook workspace add --existing-repo /path/to/repo");
        return Ok(());
    }

    println!("{}", "Workspaces:".bold());
    for ws in &workspaces {
        let mode_badge = match ws.mode {
            WorkspaceMode::GithubApp => "github-app".cyan(),
            WorkspaceMode::Pat => "pat".yellow(),
            WorkspaceMode::Ssh => "ssh".green(),
            WorkspaceMode::ExistingLocalRepo => "local".blue(),
        };
        let sync_status = if ws.auto_sync {
            "auto-sync".green()
        } else {
            "manual".dimmed()
        };
        println!(
            "  {} {} [{}] [{}] branch:{}",
            ws.id.dimmed(),
            ws.name.bold(),
            mode_badge,
            sync_status,
            ws.branch,
        );
        if let Some(ref url) = ws.remote_url {
            println!("    remote: {url}");
        }
        println!("    root: {}", ws.local_root);
    }

    Ok(())
}

pub fn show(id: &str, json: bool) -> Result<()> {
    let conn = open_db()?;
    let ws = workspace_repo::get(&conn, id)?
        .with_context(|| format!("Workspace '{id}' not found"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&ws)?);
        return Ok(());
    }

    println!("{}", ws.name.bold());
    println!("  ID:       {}", ws.id);
    println!("  Mode:     {}", ws.mode.as_str());
    println!("  Provider: {}", ws.provider.as_str());
    println!("  Branch:   {}", ws.branch);
    println!("  Root:     {}", ws.local_root);
    if let Some(ref url) = ws.remote_url {
        println!("  Remote:   {url}");
    }
    println!("  Sync:     every {}s, auto={}", ws.sync_interval_seconds, ws.auto_sync);
    println!("  Created:  {}", ws.created_at);

    // Show document and conflict counts.
    let docs = commitbook_core::storage::document_repo::list_by_workspace(&conn, &ws.id)?;
    let conflicts = commitbook_core::storage::conflict_repo::list_open(&conn, &ws.id)?;
    println!("  Docs:     {}", docs.len());
    if !conflicts.is_empty() {
        println!("  Conflicts: {}", conflicts.len().to_string().red());
    }

    Ok(())
}

pub async fn add(
    existing_repo: Option<PathBuf>,
    ssh: Option<String>,
    name: Option<String>,
    branch: Option<String>,
) -> Result<()> {
    let conn = open_db()?;
    let branch = branch.unwrap_or_else(|| "main".to_string());

    if let Some(repo_path) = existing_repo {
        add_existing_repo(&conn, &repo_path, name, &branch)?;
    } else if let Some(remote_url) = ssh {
        add_ssh(&conn, &remote_url, name, &branch)?;
    } else {
        bail!("Specify --existing-repo or --ssh. Example:\n  commitbook workspace add --existing-repo /path/to/repo");
    }

    Ok(())
}

fn add_existing_repo(
    conn: &rusqlite::Connection,
    repo_path: &PathBuf,
    name: Option<String>,
    branch: &str,
) -> Result<()> {
    let canonical = repo_path
        .canonicalize()
        .with_context(|| format!("Path not found: {}", repo_path.display()))?;

    // Verify it's a git repo.
    if !canonical.join(".git").exists() {
        bail!("{} is not a git repository", canonical.display());
    }

    // Check for auto-commit mode conflict.
    if canonical.join(".CommitBook").join("config.toml").exists() {
        println!(
            "{}",
            "Warning: This repo has auto-commit mode active (.CommitBook/config.toml)."
                .yellow()
        );
        println!("Run `commitbook stop && commitbook uninstall` first, or use a different repo.");
        bail!("Cannot add a repo that is already in auto-commit mode.");
    }

    let ws_name = name.unwrap_or_else(|| {
        canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string())
    });

    let ws = Workspace {
        id: Workspace::new_id(),
        name: ws_name.clone(),
        mode: WorkspaceMode::ExistingLocalRepo,
        provider: Provider::GenericGit,
        remote_url: None,
        owner: None,
        repo_name: None,
        branch: branch.to_string(),
        local_mode: LocalMode::Folder,
        local_root: canonical.to_string_lossy().to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    workspace_repo::insert(conn, &ws)?;

    println!("{}", "Workspace added.".green().bold());
    println!("  ID:   {}", ws.id);
    println!("  Name: {ws_name}");
    println!("  Path: {}", canonical.display());
    println!();
    println!("Next: commitbook sync {}", ws.id);

    Ok(())
}

fn add_ssh(
    conn: &rusqlite::Connection,
    remote_url: &str,
    name: Option<String>,
    branch: &str,
) -> Result<()> {
    let ws_name = name.unwrap_or_else(|| {
        remote_url
            .rsplit('/')
            .next()
            .unwrap_or("repo")
            .trim_end_matches(".git")
            .to_string()
    });

    // Clone path under ~/.commitbook/workspaces/
    let config_dir = init::commitbook_dir()?;
    let ws_id = Workspace::new_id();
    let clone_path = config_dir.join("workspaces").join(&ws_id);

    let ws = Workspace {
        id: ws_id.clone(),
        name: ws_name.clone(),
        mode: WorkspaceMode::Ssh,
        provider: Provider::GenericGit,
        remote_url: Some(remote_url.to_string()),
        owner: None,
        repo_name: None,
        branch: branch.to_string(),
        local_mode: LocalMode::Sandbox,
        local_root: clone_path.to_string_lossy().to_string(),
        merge_mode: "section_aware".to_string(),
        sync_interval_seconds: 300,
        auto_sync: true,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    workspace_repo::insert(conn, &ws)?;

    println!("{}", "SSH workspace added.".green().bold());
    println!("  ID:     {ws_id}");
    println!("  Name:   {ws_name}");
    println!("  Remote: {remote_url}");
    println!("  Clone:  {}", clone_path.display());
    println!();
    println!("Next: commitbook sync {ws_id}");

    Ok(())
}

pub fn remove(id: &str, _keep_files: bool) -> Result<()> {
    let conn = open_db()?;
    let ws = workspace_repo::get(&conn, id)?
        .with_context(|| format!("Workspace '{id}' not found"))?;

    workspace_repo::delete(&conn, id)?;

    println!("{}", "Workspace removed.".green());
    println!("  ID:   {}", ws.id);
    println!("  Name: {}", ws.name);

    Ok(())
}
