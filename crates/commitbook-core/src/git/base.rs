use anyhow::Result;

use super::GitRepo;

/// Read the base version of a file at the given SHA.
///
/// The base SHA is `SyncState.remote_head` (the point we last synced to),
/// or `origin/<branch>` on first sync. Returns `None` if the SHA is `None`
/// or if the path is not present at that commit.
pub fn read(repo: &GitRepo, base_sha: &Option<String>, path: &str) -> Option<String> {
    let sha = base_sha.as_ref()?;
    repo.show_file_at_ref(sha, path).ok()
}

/// List markdown paths at the given base SHA.
///
/// Returns an empty `Vec` if `base_sha` is `None`. Filters out files under
/// hidden directories (anything with a path segment starting with `.`) and
/// non-markdown extensions.
pub fn list(repo: &GitRepo, base_sha: &Option<String>) -> Result<Vec<String>> {
    let Some(sha) = base_sha else {
        return Ok(Vec::new());
    };
    let all = repo.ls_tree_files(sha)?;
    Ok(all.into_iter().filter(is_markdown).collect())
}

fn is_markdown(p: &String) -> bool {
    if p.split('/').any(|seg| seg.starts_with('.')) {
        return false;
    }
    let lower = p.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

#[cfg(test)]
#[path = "base_tests.rs"]
mod tests;
