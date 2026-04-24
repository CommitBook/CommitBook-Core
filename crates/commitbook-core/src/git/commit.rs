use anyhow::{Context, Result};
use git2::{Repository, Signature};
use std::collections::HashSet;
use std::path::Path;

use crate::domain::transport::{WriteFileInput, WriteFileResult};

/// Write, stage, and commit the given files in the repo at `repo_path`.
///
/// If the resulting tree matches the parent's, returns an empty `Vec` (no commit
/// is created). Otherwise returns one result per input whose content actually
/// differs from the parent tree, all sharing the new commit's SHA. Inputs
/// staged without content changes are silently dropped from the results so
/// callers see accurate "files changed" counts.
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

    // If every input shares the same message (the common case once the pipeline
    // threads a single AI-generated message through), use it. Otherwise fall
    // back to a generic count-based summary.
    let first_msg = &inputs[0].message;
    let all_same_message = inputs.iter().all(|i| &i.message == first_msg);
    let message = if all_same_message {
        first_msg.clone()
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

    // Compute the set of paths that actually changed between parent and the new
    // commit. Inputs that were staged but matched the parent exactly get
    // filtered out.
    let parent_tree = parent.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None)?;
    let mut changed: HashSet<String> = HashSet::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(p) = delta.new_file().path() {
                changed.insert(p.to_string_lossy().to_string());
            }
            if let Some(p) = delta.old_file().path() {
                changed.insert(p.to_string_lossy().to_string());
            }
            true
        },
        None,
        None,
        None,
    )?;

    Ok(inputs
        .into_iter()
        .filter(|input| changed.contains(&input.path))
        .map(|input| WriteFileResult {
            path: input.path,
            new_revision: new_revision.clone(),
        })
        .collect())
}
