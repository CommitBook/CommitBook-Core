//! Blocking libgit2 operations for CommitBook initialization/cloning. Run from
//! `tokio::task::spawn_blocking`, these can do filesystem I/O and network
//! operations that take seconds.

use std::path::Path;
use std::sync::Arc;

use commitbook_engine::commitbooks::init_dot_commitbook;
use commitbook_engine::commitbooks::publication::PublicationError;
use commitbook_engine::config::{Auth, LocalConfig};
use commitbook_engine::git::remote::{
    credential_free_url, get_remote_url, remote_identity, same_remote,
};
use commitbook_engine::git::GitRepo;
use commitbook_engine::platform::CredentialProvider;
use commitbook_engine::state::RepoLock;

use crate::errors::{CommitBookError, Result};
use crate::types::{CommitBookInput, CommitBookSummary, GitCredentialCallback};

pub fn init_local_commitbook(
    workspaces_root: &Path,
    input: &CommitBookInput,
    credential_callback: Option<Arc<dyn GitCredentialCallback>>,
) -> Result<CommitBookSummary> {
    crate::paths::validate_init_input(&input.remote_url, &input.branch)?;
    let workspaces_root = crate::paths::canonicalize_workspaces_root(workspaces_root)?;
    let existing = find_existing_clone(&workspaces_root, &input.remote_url)?;
    let (clone_path, reserved_id) = match existing {
        Some(path) => (path, None),
        None => {
            let id = allocate_local_id(&workspaces_root)?;
            (workspaces_root.join(&id), Some(id))
        }
    };
    crate::paths::validate_clone_destination(&workspaces_root, &clone_path)?;

    let creds = crate::credentials::HostCredentials::new(credential_callback);

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
        clone_repo_with_creds(&input.remote_url, &clone_path, &input.branch, &creds)?;
        let lock = RepoLock::acquire(&clone_path)
            .map_err(|error| CommitBookError::merge(format!("Repository busy: {error}")))?;
        // A fork, transfer, or stale committed config can identify a
        // different CommitBook even in a freshly-created local clone. Do not
        // publish or return an ID that disagrees with the requested remote.
        let (remote, _branch) = prepare_fresh_clone(&clone_path, input)?;
        ensure_commitbook_initialized(&clone_path, input, &remote, &creds)?;
        commitbook_engine::commitbooks::identity::ensure_locked_with_id(
            &clone_path,
            &lock,
            reserved_id.as_deref().expect("new clone has reserved ID"),
        )?;
    }

    let config = LocalConfig::load(&clone_path)
        .map_err(|e| CommitBookError::storage(format!("Failed to load config: {e}")))?;

    let remote_url = get_remote_url(&clone_path, &config.git.remote)
        .and_then(|url| credential_free_url(&url))
        .map_err(|e| CommitBookError::storage(format!("Read remote URL: {e:#}")))?;
    let provider = remote_identity(&clone_path, &config.git.remote)
        .map_err(|e| CommitBookError::storage(format!("Read remote provider: {e:#}")))?
        .provider
        .as_str()
        .to_string();
    let mode = commitbook_engine::devices::this_device(&clone_path)
        .map_err(|e| CommitBookError::storage(format!("Read this device: {e:#}")))?
        .map(|(_, device)| device.auth.as_str().to_string())
        .unwrap_or_else(|| input.mode.clone());

    Ok(CommitBookSummary {
        commitbook_local_id: commitbook_engine::commitbooks::identity::load(&clone_path)
            .map_err(|e| CommitBookError::storage(format!("Read identity: {e:#}")))?,
        remote_url,
        name: config.commitbook.name,
        mode,
        provider,
        branch: config.git.branch,
        auto_sync: true,
        doc_count: 0,
        conflict_count: 0,
    })
}

fn find_existing_clone(
    workspaces_root: &Path,
    remote_url: &str,
) -> Result<Option<std::path::PathBuf>> {
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(workspaces_root)
        .map_err(|e| CommitBookError::storage(format!("Scan workspaces: {e}")))?
    {
        let entry = entry.map_err(|e| CommitBookError::storage(format!("Read workspace: {e}")))?;
        let path = entry.path();
        if crate::paths::validate_managed_clone(workspaces_root, &path).is_err() {
            continue;
        }
        let Ok(config) = LocalConfig::load(&path) else {
            continue;
        };
        let Ok(existing_url) = get_remote_url(&path, &config.git.remote) else {
            continue;
        };
        if same_remote(&existing_url, remote_url) {
            matches.push(path);
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(CommitBookError::invalid_input(
            "Multiple local clones use this remote; choose one with its commitbook_local_id",
        )),
    }
}

fn allocate_local_id(workspaces_root: &Path) -> Result<String> {
    for _ in 0..100 {
        let id = commitbook_engine::commitbooks::identity::generate_candidate()
            .map_err(|e| CommitBookError::storage(format!("Generate local ID: {e:#}")))?;
        if local_id_available(workspaces_root, &id)? {
            return Ok(id);
        }
    }
    Err(CommitBookError::storage(
        "Could not allocate a unique local ID",
    ))
}

fn local_id_available(workspaces_root: &Path, id: &str) -> Result<bool> {
    let workspaces_root = crate::paths::canonicalize_workspaces_root(workspaces_root)?;
    commitbook_engine::commitbooks::identity::validate(id)
        .map_err(|e| CommitBookError::invalid_input(format!("Invalid local ID: {e:#}")))?;
    match std::fs::symlink_metadata(workspaces_root.join(id)) {
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CommitBookError::storage(format!(
                "Inspect local ID path: {error}"
            )))
        }
    }
    for entry in std::fs::read_dir(&workspaces_root)
        .map_err(|e| CommitBookError::storage(format!("Scan local IDs: {e}")))?
    {
        let path = entry
            .map_err(|e| CommitBookError::storage(format!("Read local ID entry: {e}")))?
            .path();
        if crate::paths::validate_managed_clone(&workspaces_root, &path).is_ok()
            && commitbook_engine::commitbooks::identity::load(&path)
                .is_ok_and(|existing| existing == id)
        {
            return Ok(false);
        }
    }
    Ok(true)
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
    match std::fs::symlink_metadata(dest) {
        Ok(_) => {
            return Err(CommitBookError::invalid_input(
                "Clone destination already exists",
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(CommitBookError::storage(format!(
                "Inspect clone destination: {error}"
            )))
        }
    }
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

    if let Err(error) = builder.clone(url, dest) {
        // This destination was absent before cloning and has not yet been
        // exposed to the app. Remove libgit2's incomplete directory so a
        // retry does not strand an unregistered ID-named clone.
        if std::fs::symlink_metadata(dest)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        {
            if let Err(cleanup) = std::fs::remove_dir_all(dest) {
                return Err(CommitBookError::storage(format!(
                    "Clone failed for {url}: {error}; remove incomplete directory {} manually: {cleanup}",
                    dest.display()
                )));
            }
        }
        return Err(CommitBookError::transport(format!(
            "Clone failed for {url}: {error}"
        )));
    }
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
    let existing_url = get_remote_url(clone_path, &remote)?;
    if !same_remote(&existing_url, &input.remote_url) {
        return Err(CommitBookError::invalid_input(format!(
            "Existing clone syncs with {} but initialization requested {}; use local registration for a separate clone",
            credential_free_url(&existing_url)?, credential_free_url(&input.remote_url)?)));
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
