use anyhow::{Context, Result};
use git2::{Repository, Signature};
use std::path::Path;

use crate::domain::transport::{WriteFileInput, WriteFileResult};

/// Write, stage, and commit the given files in the repo at `repo_path`.
///
/// If the resulting tree matches the parent's, returns an empty `Vec` (no commit
/// is created). Otherwise returns one result per input, all sharing the new
/// commit's SHA.
pub fn commit_files(
    repo_path: &Path,
    inputs: Vec<WriteFileInput>,
) -> Result<Vec<WriteFileResult>> {
    if inputs.is_empty() {
        return Ok(vec![]);
    }

    let repo = Repository::open(repo_path)
        .with_context(|| format!("Failed to open repo at {}", repo_path.display()))?;

    for input in &inputs {
        let full = repo_path.join(&input.path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, &input.content)
            .with_context(|| format!("Failed to write {}", full.display()))?;
    }

    let mut index = repo.index()?;
    for input in &inputs {
        index.add_path(Path::new(&input.path))?;
    }
    index.write()?;

    let message = if inputs.len() == 1 {
        inputs[0].message.clone()
    } else {
        format!("Update {} files", inputs.len())
    };

    let tree_oid = index.write_tree()?;
    let parent = repo.head()?.peel_to_commit()?;

    if tree_oid == parent.tree_id() {
        log::info!("Tree unchanged; skipping empty commit");
        return Ok(vec![]);
    }

    let tree = repo.find_tree(tree_oid)?;
    let sig = repo
        .signature()
        .unwrap_or_else(|_| Signature::now("CommitBook", "commitbook@local").unwrap());

    let commit_oid = repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?;
    let new_revision = commit_oid.to_string();

    Ok(inputs
        .into_iter()
        .map(|input| WriteFileResult {
            path: input.path,
            new_revision: new_revision.clone(),
        })
        .collect())
}
