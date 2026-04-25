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

#[cfg(test)]
#[path = "base_tests.rs"]
mod tests;
