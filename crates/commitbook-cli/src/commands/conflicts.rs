use anyhow::{bail, Context, Result};
use colored::Colorize;

use commitbook_core::storage::{conflict_repo, db, workspace_repo};

use super::init;

pub fn run(workspace_id: Option<&str>, json: bool) -> Result<()> {
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
        workspace_repo::list(&conn)?
    };

    let mut all_conflicts = Vec::new();

    for ws in &workspaces {
        let conflicts = conflict_repo::list_open(&conn, &ws.id)?;
        if !conflicts.is_empty() {
            all_conflicts.push((&ws.name, &ws.id, conflicts));
        }
    }

    if json {
        let output: Vec<serde_json::Value> = all_conflicts
            .iter()
            .flat_map(|(_, _, conflicts)| {
                conflicts.iter().map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "workspace_id": c.workspace_id,
                        "path": c.path,
                        "section_path": c.section_path,
                        "conflict_type": c.conflict_type.as_str(),
                        "status": c.status.as_str(),
                        "opened_at": c.opened_at,
                    })
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if all_conflicts.is_empty() {
        println!("{}", "No open conflicts.".green());
        return Ok(());
    }

    for (ws_name, ws_id, conflicts) in &all_conflicts {
        println!(
            "{} {} ({})",
            "Workspace:".bold(),
            ws_name,
            ws_id.dimmed()
        );
        for c in conflicts {
            let section_info = c
                .section_path
                .as_deref()
                .unwrap_or("(file-level)");
            println!(
                "  {} {} {} {}",
                c.id.dimmed(),
                c.path.bold(),
                section_info.cyan(),
                c.conflict_type.as_str().yellow(),
            );
        }
        println!();
    }

    let total: usize = all_conflicts.iter().map(|(_, _, c)| c.len()).sum();
    println!(
        "{} open conflict{}.",
        total.to_string().red().bold(),
        if total == 1 { "" } else { "s" }
    );

    Ok(())
}
