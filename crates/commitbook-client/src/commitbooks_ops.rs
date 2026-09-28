//! Blocking libgit2 operations for CommitBook initialization/cloning. Run from
//! `tokio::task::spawn_blocking`, these can do filesystem I/O and network
//! operations that take seconds.

use std::path::Path;

use commitbook_engine::commitbooks::publication::PublicationError;
use commitbook_engine::commitbooks::{init_dot_commitbook, slug_for};
use commitbook_engine::config::{Auth, LocalConfig};
use commitbook_engine::git::remote::remote_identity;
use commitbook_engine::git::GitRepo;
use commitbook_engine::platform::{CredentialProvider, TokenCredentials};
use commitbook_engine::state::RepoLock;

use crate::errors::{CommitBookError, Result};
use crate::types::{CommitBookInput, CommitBookSummary};

pub fn init_local_commitbook(
    workspaces_root: &Path,
    input: &CommitBookInput,
    token: &str,
) -> Result<CommitBookSummary> {
    crate::paths::validate_provider(&input.provider)?;
    crate::paths::validate_init_input(&input.owner, &input.repo, &input.branch)?;
    let workspaces_root = crate::paths::canonicalize_workspaces_root(workspaces_root)?;
    let slug = slug_for(&input.owner, &input.repo);
    let clone_path = workspaces_root.join(&slug);
    crate::paths::validate_clone_destination(&workspaces_root, &clone_path)?;

    let creds = TokenCredentials::new(token.to_string());

    if clone_path.exists() {
        // Already cloned. Repair and publish metadata while holding the same
        // repository lock used by sync and every other mutating FFI call.
        let lock = RepoLock::acquire(&clone_path)
            .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
        validate_existing_identity(&clone_path, input)?;
        let (remote, _branch) = configured_or_inferred_target(&clone_path, &input.branch)?;
        ensure_commitbook_initialized(&clone_path, input, &remote, &creds)?;
        commitbook_engine::commitbooks::identity::ensure_locked(&clone_path, &lock)?;
    } else {
        let repo_url = format!("https://github.com/{}/{}.git", input.owner, input.repo);
        clone_repo_with_creds(&repo_url, &clone_path, &input.branch, &creds)?;
        let lock = RepoLock::acquire(&clone_path)
            .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
        // A fork, transfer, or stale committed config can identify a
        // different CommitBook even in a freshly-created local clone. Do not
        // publish or return an ID that disagrees with the requested GitHub
        // repository.
        let (remote, _branch) = prepare_fresh_clone(&clone_path, input)?;
        ensure_commitbook_initialized(&clone_path, input, &remote, &creds)?;
        commitbook_engine::commitbooks::identity::ensure_locked(&clone_path, &lock)?;
    }

    let config = LocalConfig::load(&clone_path)
        .map_err(|e| CommitBookError::storage(format!("Failed to load config: {e}")))?;

    // Identity comes from the remote URL; a remote without an owner (a bare
    // local path) falls back to the requested repository.
    let (owner, repo, provider) = match remote_identity(&clone_path, &config.git.remote) {
        Ok(identity) if !identity.owner.is_empty() => (
            identity.owner,
            identity.repo,
            identity.provider.as_str().to_string(),
        ),
        _ => (
            input.owner.clone(),
            input.repo.clone(),
            input.provider.clone(),
        ),
    };
    let mode = commitbook_engine::devices::this_device(&clone_path)
        .map_err(|e| CommitBookError::storage(format!("Read this device: {e:#}")))?
        .map(|(_, device)| device.auth.as_str().to_string())
        .unwrap_or_else(|| input.mode.clone());

    Ok(CommitBookSummary {
        commitbook_id: commitbook_engine::commitbooks::identity::load(&clone_path)
            .map_err(|e| CommitBookError::storage(format!("Read identity: {e:#}")))?,
        owner,
        repo,
        name: config.commitbook.name,
        mode,
        provider,
        branch: config.git.branch,
        auto_sync: true,
        doc_count: 0,
        conflict_count: 0,
    })
}

/// A clone created by libgit2 initially names its remote `origin`. If the
/// committed CommitBook config explicitly names another remote, preserve that
/// shared setting and rename this clone's local remote to match it instead of
/// rewriting the committed config for every other clone.
fn configure_fresh_clone_target(
    clone_path: &Path,
    fallback_branch: &str,
) -> Result<(String, String)> {
    let (remote, branch) = configured_or_inferred_target(clone_path, fallback_branch)?;
    crate::paths::validate_branch(&branch)?;
    let repository = git2::Repository::open(clone_path)
        .map_err(|error| CommitBookError::storage(format!("Open fresh clone: {error}")))?;
    let cloned_remote = if remote == "origin" {
        remote.as_str()
    } else {
        "origin"
    };
    ensure_fresh_remote_branch_available(&repository, cloned_remote, &remote, &branch, clone_path)?;
    if remote != "origin" {
        let unrenamed_refspecs = repository
            .remote_rename("origin", &remote)
            .map_err(|error| {
                CommitBookError::storage(format!(
                    "Rename fresh clone remote from origin to {remote:?}: {error}"
                ))
            })?;
        if !unrenamed_refspecs.is_empty() {
            let refspecs = unrenamed_refspecs
                .iter()
                .map(|refspec| {
                    refspec
                        .map_err(|error| {
                            CommitBookError::storage(format!(
                                "Read unrenamed remote refspec as UTF-8: {error}"
                            ))
                        })?
                        .map(str::to_string)
                        .ok_or_else(|| {
                            CommitBookError::storage(
                                "An unrenamed remote refspec disappeared while reading it",
                            )
                        })
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            return Err(CommitBookError::storage(format!(
                "Renamed fresh clone remote to {remote:?}, but could not update non-default refspecs: {refspecs}"
            )));
        }
    }

    checkout_fresh_clone_branch(&repository, &remote, &branch)?;

    Ok((remote, branch))
}

fn ensure_fresh_remote_branch_available(
    repository: &git2::Repository,
    cloned_remote: &str,
    configured_remote: &str,
    branch: &str,
    clone_path: &Path,
) -> Result<()> {
    let reference = format!("refs/remotes/{cloned_remote}/{branch}");
    match repository.refname_to_id(&reference) {
        Ok(oid) => {
            repository.find_commit(oid).map_err(|error| {
                CommitBookError::storage(format!(
                    "Configured remote branch {configured_remote}/{branch} does not point to a commit: {error}"
                ))
            })?;
            Ok(())
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            Err(CommitBookError::invalid_input(format!(
                "Configured branch {branch:?} is not available on remote {configured_remote:?}; push that branch, remove the incomplete fresh clone at {}, and retry initialization",
                clone_path.display()
            )))
        }
        Err(error) => Err(CommitBookError::storage(format!(
            "Resolve configured remote branch {configured_remote}/{branch}: {error}"
        ))),
    }
}

fn checkout_fresh_clone_branch(
    repository: &git2::Repository,
    remote: &str,
    branch: &str,
) -> Result<()> {
    crate::paths::validate_branch(branch)?;
    let remote_reference = format!("refs/remotes/{remote}/{branch}");
    let remote_oid = match repository.refname_to_id(&remote_reference) {
        Ok(oid) => oid,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            return Err(CommitBookError::invalid_input(format!(
                "Configured branch {branch:?} is not available as {remote}/{branch} in the fresh clone; push that branch to the configured remote and retry initialization"
            )))
        }
        Err(error) => {
            return Err(CommitBookError::storage(format!(
                "Resolve configured remote branch {remote_reference}: {error}"
            )))
        }
    };
    let remote_commit = repository.find_commit(remote_oid).map_err(|error| {
        CommitBookError::storage(format!(
            "Load configured remote branch {remote}/{branch}: {error}"
        ))
    })?;

    let local_reference = format!("refs/heads/{branch}");
    let mut local_branch = match repository.find_branch(branch, git2::BranchType::Local) {
        Ok(local_branch) => {
            if local_branch.get().target() != Some(remote_oid) {
                return Err(CommitBookError::merge(format!(
                    "Fresh clone local branch {branch:?} does not match {remote}/{branch}; refusing to overwrite unexpected local history"
                )));
            }
            local_branch
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => repository
            .branch(branch, &remote_commit, false)
            .map_err(|error| {
                CommitBookError::storage(format!(
                    "Create local branch {branch:?} from {remote}/{branch}: {error}"
                ))
            })?,
        Err(error) => {
            return Err(CommitBookError::storage(format!(
                "Inspect fresh clone branch {branch:?}: {error}"
            )))
        }
    };

    let current_reference = repository
        .find_reference("HEAD")
        .map_err(|error| CommitBookError::storage(format!("Read fresh clone HEAD: {error}")))?
        .symbolic_target()
        .map_err(|error| {
            CommitBookError::storage(format!(
                "Read fresh clone HEAD symbolic target as UTF-8: {error}"
            ))
        })?
        .map(str::to_string)
        .ok_or_else(|| CommitBookError::merge("Fresh clone unexpectedly has detached HEAD"))?;
    if current_reference != local_reference {
        let current_commit = repository
            .head()
            .and_then(|head| head.peel_to_commit())
            .map_err(|error| {
                CommitBookError::storage(format!("Load fresh clone current branch: {error}"))
            })?;
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.safe();
        if let Err(error) = repository.checkout_tree(remote_commit.as_object(), Some(&mut checkout))
        {
            let rollback = rollback_fresh_clone_checkout(repository, &current_commit);
            return Err(CommitBookError::merge(match rollback {
                Ok(()) => format!(
                    "Cannot check out configured branch {branch:?} in the fresh clone: {error}"
                ),
                Err(rollback_error) => format!(
                    "Cannot check out configured branch {branch:?}: {error}; rollback also failed: {rollback_error}"
                ),
            }));
        }
        if let Err(error) = repository.set_head(&local_reference) {
            let rollback = rollback_fresh_clone_checkout(repository, &current_commit);
            return Err(CommitBookError::storage(match rollback {
                Ok(()) => format!("Set fresh clone HEAD to {local_reference}: {error}"),
                Err(rollback_error) => format!(
                    "Set fresh clone HEAD to {local_reference}: {error}; rollback also failed: {rollback_error}"
                ),
            }));
        }
    }

    local_branch
        .set_upstream(Some(&format!("{remote}/{branch}")))
        .map_err(|error| {
            CommitBookError::storage(format!(
                "Track configured remote branch {remote}/{branch}: {error}"
            ))
        })?;
    Ok(())
}

fn rollback_fresh_clone_checkout(
    repository: &git2::Repository,
    original_commit: &git2::Commit<'_>,
) -> std::result::Result<(), git2::Error> {
    let mut rollback = git2::build::CheckoutBuilder::new();
    rollback.force();
    repository.checkout_tree(original_commit.as_object(), Some(&mut rollback))
}

fn clone_repo_with_creds(
    url: &str,
    dest: &Path,
    branch: &str,
    creds: &dyn CredentialProvider,
) -> Result<()> {
    let config = git2::Config::open_default()
        .map_err(|error| CommitBookError::storage(format!("Open Git config: {error}")))?;
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(|url, username_from_url, allowed| {
        creds
            .provide(&config, url, username_from_url, allowed)
            .map_err(|e| git2::Error::from_str(&format!("credential provider failed: {e}")))
    });

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);
    // Check out the requested branch so a local `refs/heads/<branch>` exists;
    // otherwise the later push of `<branch>:<branch>` has no matching source ref.
    builder.branch(branch);

    builder
        .clone(url, dest)
        .map_err(|e| CommitBookError::transport(format!("Clone failed for {url}: {e}")))?;
    Ok(())
}

/// Repair `.CommitBook` metadata, commit only those paths, and push. This is
/// idempotent and preserves unrelated staged changes in an existing clone.
fn ensure_commitbook_initialized(
    clone_path: &Path,
    input: &CommitBookInput,
    remote: &str,
    creds: &dyn CredentialProvider,
) -> Result<()> {
    let repo = GitRepo::open(clone_path)
        .map_err(|e| CommitBookError::storage(format!("Open clone: {e}")))?;
    if repo.merge_in_progress() {
        return Err(CommitBookError::merge(
            "Cannot initialize CommitBook while a merge is in progress; resolve or abort the merge first",
        ));
    }
    let expected_branch = if LocalConfig::exists(clone_path) {
        LocalConfig::load(clone_path)
            .map_err(|error| CommitBookError::storage(format!("Load config: {error}")))?
            .git
            .branch
    } else {
        input.branch.clone()
    };
    let current_branch = repo
        .current_branch()
        .map_err(|error| CommitBookError::merge(format!("Read current branch: {error}")))?;
    if current_branch != expected_branch {
        return Err(CommitBookError::invalid_input(format!(
            "Cannot publish CommitBook metadata: checked out branch {current_branch:?} does not match configured branch {expected_branch:?}"
        )));
    }
    let auth: Auth = input
        .mode
        .parse()
        .map_err(|error| CommitBookError::invalid_input(format!("{error:#}")))?;
    init_dot_commitbook(
        clone_path,
        &input.name,
        &input.branch,
        remote,
        input.device_name.as_deref(),
        auth,
    )
    .map_err(|e| CommitBookError::storage(format!("init_dot_commitbook: {e}")))?;

    let mut config = LocalConfig::load(clone_path)
        .map_err(|e| CommitBookError::storage(format!("Load config: {e}")))?;
    if config.git.remote != remote {
        config.git.remote = remote.to_string();
        config
            .save(clone_path)
            .map_err(|e| CommitBookError::storage(format!("Save remote: {e}")))?;
    }
    commitbook_engine::commitbooks::publication::publish_metadata(
        &repo,
        &config.git.remote,
        &config.git.branch,
        "Initialize CommitBook",
        creds,
    )
    .map_err(|error| match error {
        PublicationError::Push(_) => CommitBookError::transport(format!(
            "Metadata was committed locally, but push failed: {error}. The local metadata commit remains intact; run sync to reconcile a non-fast-forward remote, or fix the remote, authentication, or connectivity and retry initialization"
        )),
        PublicationError::Commit(_) => {
            CommitBookError::storage(format!("Commit metadata: {error}"))
        }
        PublicationError::State(_) => {
            CommitBookError::storage(format!("Record metadata publication: {error}"))
        }
    })?;
    Ok(())
}

/// Validate identity, switch the clone to the configured target, then validate
/// again: the configured branch may carry `[commitbook]` metadata that names a
/// different repository than the branch the clone was identity-checked on.
fn prepare_fresh_clone(clone_path: &Path, input: &CommitBookInput) -> Result<(String, String)> {
    validate_existing_identity(clone_path, input)?;
    let target = configure_fresh_clone_target(clone_path, &input.branch)?;
    validate_existing_identity(clone_path, input)?;
    Ok(target)
}

fn validate_existing_identity(clone_path: &Path, input: &CommitBookInput) -> Result<()> {
    // Existing clones must match the selected remote, including its host.
    // Fresh clones can temporarily name the configured remote `origin`.
    let repository = git2::Repository::open(clone_path)
        .map_err(|error| CommitBookError::storage(format!("Open clone: {error}")))?;
    let configured = if LocalConfig::exists(clone_path) {
        Some(LocalConfig::load(clone_path)?.git.remote)
    } else {
        None
    };
    let remote = match configured {
        Some(name) if repository.find_remote(&name).is_ok() => name,
        _ => {
            let names = repository
                .remotes()
                .map_err(|e| CommitBookError::storage(format!("List remotes: {e}")))?;
            let names: Vec<_> = names.iter().flatten().flatten().collect();
            if names.len() != 1 {
                return Err(CommitBookError::invalid_input(
                    "Existing clone must have exactly one remote or a valid configured remote",
                ));
            }
            names[0].to_string()
        }
    };
    let identity = remote_identity(clone_path, &remote)?;
    if !matches!(
        identity.host.as_deref(),
        Some("github.com" | "www.github.com")
    ) || !identity.owner.eq_ignore_ascii_case(&input.owner)
        || !identity.repo.eq_ignore_ascii_case(&input.repo)
    {
        return Err(CommitBookError::invalid_input(format!(
            "Existing clone syncs with {}/{}/{} but initialization requested github.com/{}/{}; use local registration for an existing non-GitHub clone",
            identity.host.as_deref().unwrap_or("local"), identity.owner, identity.repo, input.owner, input.repo)));
    }
    Ok(())
}

fn configured_or_inferred_target(
    clone_path: &Path,
    fallback_branch: &str,
) -> Result<(String, String)> {
    let config_path = LocalConfig::config_path(clone_path);
    match std::fs::symlink_metadata(&config_path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            let config = LocalConfig::load(clone_path)
                .map_err(|error| CommitBookError::storage(format!("Load config: {error}")))?;
            return Ok((config.git.remote, config.git.branch));
        }
        Ok(_) => {
            return Err(CommitBookError::invalid_input(format!(
                "CommitBook config is not a regular file: {}",
                config_path.display()
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CommitBookError::storage(format!(
                "Inspect config {}: {error}",
                config_path.display()
            )))
        }
    }

    let repository = git2::Repository::open(clone_path)
        .map_err(|error| CommitBookError::storage(format!("Open clone: {error}")))?;
    let remotes = repository
        .remotes()
        .map_err(|error| CommitBookError::storage(format!("List remotes: {error}")))?;
    let remote = match remotes.len() {
        1 => remotes
            .get(0)
            .map_err(|error| {
                CommitBookError::invalid_input(format!(
                    "The only Git remote name is not valid UTF-8: {error}"
                ))
            })?
            .map(ToOwned::to_owned)
            .ok_or_else(|| CommitBookError::invalid_input("The only Git remote has no name"))?,
        0 => {
            return Err(CommitBookError::invalid_input(
                "Existing clone has no CommitBook config and no Git remote; add exactly one remote and retry",
            ))
        }
        count => {
            return Err(CommitBookError::invalid_input(format!(
                "Existing clone has no CommitBook config and {count} Git remotes; leave exactly one remote or create .CommitBook/config.toml"
            )))
        }
    };
    Ok((remote, fallback_branch.to_string()))
}

#[cfg(test)]
#[path = "commitbooks_ops_tests.rs"]
mod tests;
