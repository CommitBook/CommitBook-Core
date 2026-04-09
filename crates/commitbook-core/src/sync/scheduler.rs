use anyhow::Result;
use rusqlite::Connection;

use crate::domain::transport::RemoteTransport;
use crate::domain::workspace::Workspace;
use crate::storage::workspace_repo;

use super::pipeline;
use super::planner;

/// Run a single sync cycle for a workspace.
pub async fn sync_workspace(
    workspace: &Workspace,
    transport: &dyn RemoteTransport,
    conn: &Connection,
) -> Result<pipeline::SyncResult> {
    let plan = planner::create_sync_plan(workspace, transport, conn).await?;
    let result = pipeline::execute_sync(&plan, workspace, transport, conn).await?;
    Ok(result)
}

/// Run sync for all workspaces that have auto_sync enabled.
pub async fn sync_all_due(
    conn: &Connection,
    transport_factory: &dyn Fn(&Workspace) -> Option<Box<dyn RemoteTransport>>,
) -> Result<Vec<(String, Result<pipeline::SyncResult>)>> {
    let workspaces = workspace_repo::list(conn)?;
    let mut results = Vec::new();

    for ws in &workspaces {
        if !ws.auto_sync {
            continue;
        }

        if let Some(transport) = transport_factory(ws) {
            let result = sync_workspace(ws, transport.as_ref(), conn).await;
            results.push((ws.id.clone(), result));
        }
    }

    Ok(results)
}
