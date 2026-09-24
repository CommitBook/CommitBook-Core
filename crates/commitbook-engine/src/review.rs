//! Shared conflict actions and durable, opt-in AI proposals. All writes require RepoLock.
use crate::{
    ai::{ConflictResolution, ConflictResolver},
    config::LocalConfig,
    git::{GitConflict, GitRepo},
    state::RepoLock,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideIdentity {
    pub oid: Option<String>,
    pub mode: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub path: String,
    pub revision: String,
    pub head: String,
    pub merge_parents: String,
    pub sides: Vec<SideIdentity>,
    pub content: Option<String>,
    pub provider: String,
    pub created_at: String,
    pub rejected: bool,
    #[serde(default)]
    pub generation: u64,
}
#[derive(Debug, Serialize, Deserialize)]
struct Store {
    version: u32,
    proposals: Vec<Proposal>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            version: 1,
            proposals: vec![],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictView {
    pub path: String,
    pub conflict_type: String,
    pub binary: bool,
    pub ancestor: Option<String>,
    pub local: Option<String>,
    pub remote: Option<String>,
    pub revision: String,
    pub proposal: Option<Proposal>,
    pub proposal_stale: bool,
    pub proposal_version: Option<String>,
    pub ancestor_present: bool,
    pub local_present: bool,
    pub remote_present: bool,
}
#[derive(Debug, Deserialize)]
pub struct ResolutionInput {
    pub proposal_version: Option<String>,
    pub path: String,
    pub revision: String,
    pub action: String,
    pub content: Option<String>,
}
#[derive(Debug)]
pub enum ReviewError {
    Stale,
    Missing,
}
impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Stale => "Conflict changed; refresh before resolving",
            Self::Missing => "Conflict or proposal no longer exists",
        })
    }
}
impl std::error::Error for ReviewError {}
fn proposal_version(proposal: &Proposal) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(proposal)?)))
}
fn check_proposal_version(proposal: &Proposal, input: &ResolutionInput) -> Result<()> {
    anyhow::ensure!(
        input.proposal_version.as_deref() == Some(proposal_version(proposal)?.as_str()),
        ReviewError::Stale
    );
    Ok(())
}
fn load(root: &Path) -> Result<Store> {
    let path = LocalConfig::local_dir(root).join("conflict-proposals.toml");
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => (),
        Ok(_) => bail!("Proposal store is not a regular file"),
        Err(e) => return Err(e.into()),
    }
    let store: Store = toml::from_str(&std::fs::read_to_string(path)?)?;
    anyhow::ensure!(store.version == 1, "Unsupported conflict proposal version");
    Ok(store)
}
fn save(root: &Path, store: &Store) -> Result<()> {
    let dir = LocalConfig::local_dir(root);
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    file.write_all(toml::to_string(store)?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(dir.join("conflict-proposals.toml"))?;
    Ok(())
}
fn identity(root: &Path) -> Result<(String, String)> {
    let repo = git2::Repository::open(root)?;
    let head = repo.head()?.peel_to_commit()?.id().to_string();
    let parents = std::fs::read_to_string(repo.path().join("MERGE_HEAD"))?;
    Ok((head, parents))
}
fn revision(root: &Path, conflict: &GitConflict) -> Result<String> {
    let (head, parents) = identity(root)?;
    let mut h = Sha256::new();
    h.update(serde_json::to_vec(&(
        head,
        parents,
        &conflict.path,
        sides(conflict),
    ))?);
    let mut path = root.to_path_buf();
    let components: Vec<_> = Path::new(&conflict.path).components().collect();
    for (i, component) in components.iter().enumerate() {
        anyhow::ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "Unsafe conflict path"
        );
        path.push(component.as_os_str());
        if i + 1 < components.len() {
            match std::fs::symlink_metadata(&path) {
                Ok(m) => anyhow::ensure!(
                    m.is_dir() && !m.file_type().is_symlink(),
                    "Unsafe conflict parent"
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
    }
    match std::fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_symlink() => {
            h.update(b"symlink");
            h.update(std::fs::read_link(path)?.as_os_str().as_encoded_bytes());
        }
        Ok(m) if m.is_file() => {
            h.update(b"file");
            h.update(std::fs::read(path)?);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                h.update(m.permissions().mode().to_le_bytes());
            }
        }
        Ok(m) if m.is_dir() => {
            h.update(b"directory");
        }
        Ok(_) => bail!("Unsupported conflict worktree entry"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => h.update(b"deleted"),
        Err(e) => return Err(e.into()),
    }
    Ok(hex::encode(h.finalize()))
}
fn sides(c: &GitConflict) -> Vec<SideIdentity> {
    [&c.ancestor, &c.local, &c.remote]
        .iter()
        .map(|s| SideIdentity {
            oid: s.as_ref().map(|s| s.oid.to_string()),
            mode: s.as_ref().map(|s| s.mode),
        })
        .collect()
}
pub fn list(root: &Path) -> Result<Vec<ConflictView>> {
    let repo = GitRepo::open(root)?;
    let store = load(root)?;
    repo.list_conflicts_structured()?
        .into_iter()
        .map(|c| {
            let rev = revision(root, &c)?;
            let proposal = store.proposals.iter().find(|p| p.path == c.path).cloned();
            Ok(ConflictView {
                conflict_type: c.classification().into(),
                binary: c.is_binary_or_special(),
                ancestor: c.ancestor_text().map(str::to_owned),
                local: c.local_text().map(str::to_owned),
                remote: c.remote_text().map(str::to_owned),
                ancestor_present: c.ancestor.is_some(),
                local_present: c.local.is_some(),
                remote_present: c.remote.is_some(),
                proposal_version: proposal.as_ref().map(proposal_version).transpose()?,
                proposal_stale: proposal.as_ref().is_some_and(|p| p.revision != rev),
                path: c.path,
                revision: rev,
                proposal,
            })
        })
        .collect()
}
pub fn cleanup_locked(root: &Path, lock: &RepoLock) -> Result<()> {
    lock.ensure_matches(root)?;
    let paths = GitRepo::open(root)?.list_conflicted_paths()?;
    let mut store = load(root)?;
    let before = store.proposals.len();
    store.proposals.retain(|p| paths.contains(&p.path));
    if before != store.proposals.len() {
        save(root, &store)?;
    }
    Ok(())
}
fn check(root: &Path, branch: &str) -> Result<GitRepo> {
    let repo = GitRepo::open(root)?;
    anyhow::ensure!(
        repo.current_branch()? == branch,
        "Check out the configured branch {branch:?} before resolving"
    );
    anyhow::ensure!(repo.merge_in_progress(), "No merge is in progress");
    Ok(repo)
}
pub fn apply(root: &Path, branch: &str, input: &ResolutionInput) -> Result<()> {
    let lock = RepoLock::acquire(root)?;
    apply_locked(root, branch, input, &lock)
}
pub fn apply_locked(
    root: &Path,
    branch: &str,
    input: &ResolutionInput,
    lock: &RepoLock,
) -> Result<()> {
    lock.ensure_matches(root)?;
    let repo = check(root, branch)?;
    let c = repo
        .find_conflict(&input.path)?
        .ok_or(ReviewError::Missing)?;
    anyhow::ensure!(revision(root, &c)? == input.revision, ReviewError::Stale);
    let mut store = load(root)?;
    let mut content = input.content.clone();
    let mut action = input.action.as_str();
    if action == "accept" {
        let p = store
            .proposals
            .iter()
            .find(|p| p.path == input.path)
            .ok_or(ReviewError::Missing)?;
        anyhow::ensure!(
            p.revision == input.revision && !p.rejected,
            ReviewError::Stale
        );
        check_proposal_version(p, input)?;
        content = p.content.clone();
        action = if content.is_some() {
            "manual_edit"
        } else {
            "delete"
        };
    }
    match action {
        "take_local" => repo.resolve_conflict_with_side(&c.path, c.local.as_ref())?,
        "take_remote" => repo.resolve_conflict_with_side(&c.path, c.remote.as_ref())?,
        "delete" => repo.resolve_conflict_with_side(&c.path, None)?,
        "keep_both" => {
            anyhow::ensure!(
                !c.is_binary_or_special(),
                "Choose a side for binary or special conflicts"
            );
            repo.resolve_conflict_with_text(
                &c.path,
                &[c.local_text(), c.remote_text()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("\n"),
            )?;
        }
        "manual_edit" => repo.resolve_conflict_with_text(
            &c.path,
            content.as_deref().context("manual_edit requires content")?,
        )?,
        _ => bail!("Unknown conflict action: {action}"),
    }
    store.proposals.retain(|p| p.path != c.path);
    save(root, &store)?;
    if repo.list_conflicted_paths()?.is_empty() {
        repo.finalize_merge_commit_on_branch(None, branch)?;
    }
    Ok(())
}
pub async fn propose_locked(
    root: &Path,
    path: &str,
    expected: &str,
    resolver: &dyn ConflictResolver,
    lock: &RepoLock,
) -> Result<()> {
    lock.ensure_matches(root)?;
    let repo = GitRepo::open(root)?;
    let c = repo.find_conflict(path)?.ok_or(ReviewError::Missing)?;
    anyhow::ensure!(revision(root, &c)? == expected, ReviewError::Stale);
    anyhow::ensure!(
        !c.is_binary_or_special(),
        "Choose a side for binary or special conflicts"
    );
    let result = resolver.resolve(&c, root).await?;
    let current = repo.find_conflict(path)?.ok_or(ReviewError::Missing)?;
    anyhow::ensure!(revision(root, &current)? == expected, ReviewError::Stale);
    let content = match result {
        ConflictResolution::WriteContent(text) => {
            anyhow::ensure!(
                !crate::git::conflicts::has_conflict_markers(&text),
                "AI result still contains conflict markers"
            );
            Some(text)
        }
        ConflictResolution::DeleteFile => None,
    };
    let (head, merge_parents) = identity(root)?;
    let mut store = load(root)?;
    let generation = store
        .proposals
        .iter()
        .find(|p| p.path == path)
        .map_or(0, |p| p.generation)
        .checked_add(1)
        .context("Proposal generation overflow")?;
    store.proposals.retain(|p| p.path != path);
    store.proposals.push(Proposal {
        path: path.into(),
        revision: expected.into(),
        head,
        merge_parents,
        sides: sides(&c),
        content,
        provider: resolver.key().into(),
        created_at: crate::utils::datetime::now_iso(),
        rejected: false,
        generation,
    });
    save(root, &store)
}
pub async fn proposal_action(root: &Path, input: &ResolutionInput) -> Result<()> {
    let lock = RepoLock::acquire(root)?;
    let config = LocalConfig::load(root)?;
    let repo = check(root, &config.git.branch)?;
    let c = repo
        .find_conflict(&input.path)?
        .ok_or(ReviewError::Missing)?;
    anyhow::ensure!(revision(root, &c)? == input.revision, ReviewError::Stale);
    match input.action.as_str() {
        "accept" => apply_locked(root, &config.git.branch, input, &lock),
        "reject" => {
            let mut store = load(root)?;
            let p = store
                .proposals
                .iter_mut()
                .find(|p| p.path == input.path)
                .ok_or(ReviewError::Missing)?;
            check_proposal_version(p, input)?;
            p.rejected = true;
            save(root, &store)
        }
        "regenerate" => {
            let registry = crate::ai::ResolverRegistry::new();
            let resolver = registry
                .get(config.conflicts.agent.as_str())
                .context("Configured AI resolver is unavailable")?;
            propose_locked(root, &input.path, &input.revision, resolver, &lock).await
        }
        _ => bail!("Unknown proposal action"),
    }
}
/// Returns true when review blocks sync. A stored proposal always blocks automatic application.
pub async fn prepare_locked(
    root: &Path,
    enabled: bool,
    resolver: Option<&dyn ConflictResolver>,
    lock: &RepoLock,
) -> Result<bool> {
    cleanup_locked(root, lock)?;
    let conflicts = list(root)?;
    if conflicts.is_empty() {
        return Ok(false);
    }
    let blocked = enabled || conflicts.iter().any(|c| c.proposal.is_some());
    if blocked && enabled {
        if let Some(resolver) = resolver {
            for c in conflicts
                .iter()
                .filter(|c| !c.binary && c.proposal.is_none())
            {
                propose_locked(root, &c.path, &c.revision, resolver, lock).await?;
            }
        }
    }
    Ok(blocked)
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
