use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

use commitbook_engine::config::values::ANY_AGENT_ORDER;
use commitbook_engine::config::{CommitMode, ConflictMode, LocalConfig};
use commitbook_engine::cron;
use commitbook_engine::git::GitRepo;
use commitbook_engine::state::auth::AuthConfig;
use commitbook_engine::state::sync_state::SyncState;
use commitbook_engine::state::RepoLock;

struct Check {
    name: &'static str,
    status: &'static str,
    failed: bool,
    details: Vec<String>,
}

impl Check {
    fn new(name: &'static str, status: &'static str, failed: bool, details: Vec<String>) -> Self {
        Self {
            name,
            status,
            failed,
            details,
        }
    }
}

#[derive(Default)]
struct DoctorReport {
    checks: Vec<Check>,
    repairs: Vec<Check>,
}

impl DoctorReport {
    fn ok(&self) -> bool {
        !self
            .checks
            .iter()
            .chain(&self.repairs)
            .any(|check| check.failed)
    }

    fn json(&self) -> Result<String> {
        let items = |checks: &[Check]| {
            checks
                .iter()
                .map(|check| {
                    serde_json::json!({
                        "name": check.name,
                        "status": check.status,
                        "failed": check.failed,
                        "details": check.details,
                    })
                })
                .collect::<Vec<_>>()
        };
        Ok(serde_json::to_string_pretty(&serde_json::json!({
            "ok": self.ok(),
            "checks": items(&self.checks),
            "repairs": items(&self.repairs),
        }))?)
    }

    fn print_text(&self, fix: bool) {
        println!("{}", "CommitBook Doctor".bold().cyan());
        println!();
        for check in &self.checks {
            let status = if check.failed {
                check.status.red().bold().to_string()
            } else if check.status == "ok" || check.status == "running" {
                check.status.green().bold().to_string()
            } else {
                check.status.yellow().to_string()
            };
            println!("  {}... {status}", check.name);
            for detail in &check.details {
                println!("    {detail}");
            }
        }
        if fix {
            println!();
            println!("  {}", "Auto-repair:".bold().cyan());
            if self.repairs.is_empty() {
                println!("    Nothing to repair.");
            }
            for repair in &self.repairs {
                println!("    {}... {}", repair.name, repair.status);
                for detail in &repair.details {
                    println!("      {detail}");
                }
            }
            if self.repairs.iter().any(|repair| repair.status == "applied") {
                println!("  Repairs applied. Re-run `commitbook doctor` to verify.");
            }
        }
        println!();
        if self.ok() {
            println!("  {}", "All checks passed.".green().bold());
        } else {
            println!("  {}", "Some checks failed. See above.".red().bold());
            if !fix {
                println!("  Try `commitbook doctor --fix` to auto-repair common issues.");
            }
        }
    }
}

pub fn run(cb_dir: &Path, repo_root: &Path, json: bool, fix: bool) -> Result<()> {
    let report = build_report(cb_dir, repo_root, fix);

    if json {
        println!("{}", report.json()?);
    } else {
        report.print_text(fix);
    }

    if report.ok() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("doctor reported one or more failures"))
    }
}

fn build_report(cb_dir: &Path, repo_root: &Path, fix: bool) -> DoctorReport {
    let mut report = diagnose(cb_dir, repo_root);
    if fix {
        let logs_were_missing = !LocalConfig::logs_dir(repo_root).is_dir();
        match RepoLock::acquire(repo_root) {
            Ok(_lock) => {
                report.repairs = run_fixes(repo_root);
                // RepoLock creates the local layout before taking its file
                // lock, so it may have already recreated logs/ for this run.
                if logs_were_missing
                    && LocalConfig::logs_dir(repo_root).is_dir()
                    && !report
                        .repairs
                        .iter()
                        .any(|repair| repair.name == "Creating logs/")
                {
                    report
                        .repairs
                        .insert(0, Check::new("Creating logs/", "applied", false, vec![]));
                }
            }
            Err(error) => report.repairs.push(Check::new(
                "Repository lock",
                "failed",
                true,
                vec![format!("{error:#}")],
            )),
        }
    }
    report
}

fn diagnose(cb_dir: &Path, repo_root: &Path) -> DoctorReport {
    let mut report = DoctorReport::default();
    let config = LocalConfig::load_read_only(repo_root);

    let git_installed = Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    report.checks.push(Check::new(
        "Git installed",
        if git_installed { "ok" } else { "failed" },
        !git_installed,
        vec![],
    ));

    let is_repo = GitRepo::is_repo(repo_root);
    report.checks.push(Check::new(
        "Git repository",
        if is_repo { "ok" } else { "failed" },
        !is_repo,
        if is_repo {
            vec![]
        } else {
            vec!["Not a git repository.".into()]
        },
    ));

    let filters = match commitbook_engine::git::attributes::unsupported_filters(repo_root) {
        Ok(filters) if filters.is_empty() => Check::new("Git filters", "ok", false, vec![]),
        Ok(filters) => {
            let mut details = filters.iter().map(ToString::to_string).collect::<Vec<_>>();
            details.push(
                "Sync refuses to commit here: these filters would be skipped and files pushed unfiltered."
                    .into(),
            );
            Check::new("Git filters", "unsupported", true, details)
        }
        Err(error) => Check::new("Git filters", "skip", false, vec![format!("{error:#}")]),
    };
    report.checks.push(filters);

    let remote = match commitbook_engine::git::remote::list_remote_names(repo_root) {
        Ok(names) if names.len() == 1 => {
            let actual = &names[0];
            match config.as_ref().ok() {
                Some(cfg) if cfg.git.remote != *actual => Check::new(
                    "Git remote",
                    "mismatch",
                    true,
                    vec![format!(
                        "Config says `{}`, git has `{}`. Re-run `commitbook init`.",
                        cfg.git.remote, actual
                    )],
                ),
                _ => Check::new("Git remote", "ok", false, vec![actual.clone()]),
            }
        }
        Ok(names) if names.is_empty() => Check::new(
            "Git remote",
            "failed",
            true,
            vec!["No remote configured. CommitBook requires exactly 1 remote.".into()],
        ),
        Ok(names) => Check::new(
            "Git remote",
            "failed",
            true,
            vec![format!(
                "Found {} remotes ({}). CommitBook requires exactly 1.",
                names.len(),
                names.join(", ")
            )],
        ),
        Err(error) => Check::new("Git remote", "skip", false, vec![format!("{error:#}")]),
    };
    report.checks.push(remote);

    let directory_exists = cb_dir.is_dir();
    report.checks.push(Check::new(
        ".CommitBook/ directory",
        if directory_exists { "ok" } else { "failed" },
        !directory_exists,
        vec![],
    ));

    // Pre-release files never block sync (it never adds them), so they warn.
    let contents = match commitbook_engine::state::legacy_metadata_entries(repo_root) {
        Ok(entries) if entries.is_empty() => {
            Check::new(".CommitBook/ contents", "ok", false, vec![])
        }
        Ok(entries) => Check::new(
            ".CommitBook/ contents",
            "old files",
            false,
            vec![commitbook_engine::state::legacy_metadata_warning(&entries)],
        ),
        Err(error) => Check::new(
            ".CommitBook/ contents",
            "skip",
            false,
            vec![format!("{error:#}")],
        ),
    };
    report.checks.push(contents);

    let config_check = if !LocalConfig::exists(repo_root) {
        Check::new("config.toml", "missing", true, vec![])
    } else {
        match config.as_ref() {
            Ok(_) => Check::new("config.toml", "ok", false, vec![]),
            Err(error) => Check::new("config.toml", "invalid", true, vec![format!("{error:#}")]),
        }
    };
    report.checks.push(config_check);

    let auth = match AuthConfig::load(cb_dir) {
        Ok(auth) if auth.has_token() => Check::new("Token auth (optional)", "ok", false, vec![]),
        Ok(_) => Check::new(
            "Token auth (optional)",
            "not configured",
            false,
            vec![
                "Normal for desktop git sync; `commitbook token set` is only needed for token-backed transports."
                    .into(),
            ],
        ),
        Err(_) => Check::new(
            "Token auth (optional)",
            "failed",
            true,
            vec!["Error reading auth.toml.".into()],
        ),
    };
    report.checks.push(auth);

    let scheduler = cron::health(repo_root);
    let last_attempt = SyncState::load(cb_dir)
        .ok()
        .and_then(|state| state.last_attempt_at);
    let warning = scheduler.warning(
        config.as_ref().ok().map(|cfg| cfg.sync.schedule.as_str()),
        last_attempt.as_deref(),
        chrono::Utc::now(),
    );
    let mut scheduler_details = warning.into_iter().collect::<Vec<_>>();
    if let Some(binary) = cron::scheduled_binary(repo_root)
        .filter(|binary| scheduler.is_loaded() && cron::is_transient_binary(binary))
    {
        scheduler_details.push(format!(
            "Scheduler runs a build artifact ({}); it stops working when that build is removed.",
            binary.display()
        ));
    }
    let (status, failed) = match scheduler {
        cron::SchedulerHealth::Broken(_) => ("broken", true),
        cron::SchedulerHealth::Running if !scheduler_details.is_empty() => ("stale", false),
        cron::SchedulerHealth::Running => ("running", false),
        cron::SchedulerHealth::Stopped => ("stopped", false),
    };
    report
        .checks
        .push(Check::new("Scheduler", status, failed, scheduler_details));

    let agents = match config.as_ref() {
        Ok(config) => {
            let commit_ai = config.commit.mode == CommitMode::Ai;
            let conflict_ai = matches!(
                config.conflicts.mode,
                ConflictMode::Ai | ConflictMode::Review
            );
            if !commit_ai && !conflict_ai {
                Check::new("AI agents", "not used", false, vec![])
            } else {
                let mut details = Vec::new();
                if commit_ai {
                    let agents = match config.commit.agent.agent() {
                        Some(agent) => vec![agent],
                        None => ANY_AGENT_ORDER.to_vec(),
                    };
                    let keys = agents
                        .iter()
                        .map(|agent| agent.commit_provider_key().to_string())
                        .collect::<Vec<_>>();
                    let available = commitbook_engine::ai::ProviderChain::new()
                        .check_availability(&keys)
                        .into_iter()
                        .filter(|(_, _, installed)| *installed)
                        .map(|(_, name, _)| name)
                        .collect::<Vec<_>>();
                    details.push(format!(
                        "Commit messages ({}): {}",
                        config.commit.agent,
                        if available.is_empty() {
                            "not installed; commit messages use timestamp text".to_string()
                        } else {
                            available.join(", ")
                        }
                    ));
                }
                if conflict_ai {
                    let installed = commitbook_engine::ai::ResolverRegistry::new()
                        .get(config.conflicts.agent.as_str())
                        .is_some();
                    details.push(format!(
                        "Conflicts ({}, {}): {}",
                        config.conflicts.mode,
                        config.conflicts.agent,
                        if installed {
                            "installed"
                        } else {
                            "not installed; conflicts are left for manual resolution"
                        }
                    ));
                }
                Check::new("AI agents", "checked", false, details)
            }
        }
        Err(_) => Check::new(
            "AI agents",
            "unknown",
            false,
            vec!["Configuration is unreadable.".into()],
        ),
    };
    report.checks.push(agents);

    let checkpoint = match SyncState::load(cb_dir) {
        Ok(state) if state.last_sync_at.is_some() => {
            Check::new("Sync checkpoint", "ok", false, vec![])
        }
        _ => Check::new(
            "Sync checkpoint",
            "not yet established",
            false,
            vec!["Will be set after the first successful sync.".into()],
        ),
    };
    report.checks.push(checkpoint);

    if let Ok(repo) = GitRepo::open(repo_root) {
        if repo.has_remote() {
            let branch = config
                .as_ref()
                .ok()
                .map(|cfg| cfg.git.branch.as_str())
                .unwrap_or("main");
            let remote_name = config
                .as_ref()
                .ok()
                .map(|cfg| cfg.git.remote.as_str())
                .unwrap_or("origin");
            let remote_ref = format!("{remote_name}/{branch}");
            let mut divergence = match repo.ahead_behind("HEAD", &remote_ref) {
                Ok((0, 0)) => Check::new("Local vs remote", "in sync", false, vec![]),
                Ok((ahead, 0)) => Check::new(
                    "Local vs remote",
                    "ahead",
                    false,
                    vec![format!("{ahead} commit(s) ahead. Next sync will try to push.")],
                ),
                Ok((0, behind)) => Check::new(
                    "Local vs remote",
                    "behind",
                    false,
                    vec![format!("{behind} commit(s) behind. Run: git pull --ff-only")],
                ),
                Ok((ahead, behind)) => Check::new(
                    "Local vs remote",
                    "diverged",
                    true,
                    vec![
                        format!("{ahead} ahead, {behind} behind: local and {remote_name} have different histories."),
                        format!("Inspect: git fetch {remote_name}"),
                        format!("Compare: git diff {remote_ref} -- '*.md' '*.markdown'"),
                        format!("If the diff is empty/expected: git reset --hard {remote_ref}"),
                        format!("Otherwise merge by hand: git merge {remote_ref}"),
                    ],
                ),
                Err(_) => Check::new(
                    "Local vs remote",
                    "skip",
                    false,
                    vec!["Could not determine divergence (fetch may have failed).".into()],
                ),
            };
            divergence
                .details
                .insert(0, format!("Reference: {remote_ref}"));
            report.checks.push(divergence);
        }
    }

    report
}

/// Only called while holding the repository mutation lock.
fn run_fixes(repo_root: &Path) -> Vec<Check> {
    [fix_logs_dir(repo_root), fix_plist_binary_path(repo_root)]
        .into_iter()
        .flatten()
        .collect()
}

/// Recreate `.CommitBook/local/logs/` if missing. Cheap and idempotent.
fn fix_logs_dir(repo_root: &Path) -> Option<Check> {
    let logs = LocalConfig::logs_dir(repo_root);
    if logs.is_dir() {
        return None;
    }
    Some(match std::fs::create_dir_all(&logs) {
        Ok(()) => Check::new("Creating logs/", "applied", false, vec![]),
        Err(error) => Check::new("Creating logs/", "failed", true, vec![error.to_string()]),
    })
}

/// If a launchd plist exists for this repo and points at a stale binary path
/// (e.g., a workspace that no longer exists, or a target/debug from a
/// different checkout), reinstall it pointing at the binary actually running
/// `commitbook doctor` right now.
#[cfg(not(target_os = "macos"))]
fn fix_plist_binary_path(_repo_root: &Path) -> Option<Check> {
    None
}

#[cfg(target_os = "macos")]
fn fix_plist_binary_path(repo_root: &Path) -> Option<Check> {
    let current_exe = commitbook_engine::settings::current_binary();

    if !cron::is_loaded(repo_root) {
        return None;
    }
    let binary_is_current =
        std::fs::read_to_string(commitbook_engine::cron::macos::plist_path(repo_root))
            .is_ok_and(|contents| contents.contains(&current_exe.to_string_lossy().to_string()));
    if binary_is_current {
        return None;
    }

    let mut details = Vec::new();
    if cron::is_transient_binary(&current_exe) {
        details.push(format!(
            "Warning: {} is a build artifact; install `commitbook` (e.g. `cargo install --path crates/commitbook-cli`) and rerun `doctor --fix` from it.",
            current_exe.display()
        ));
    }
    let config = match LocalConfig::load(repo_root) {
        Ok(c) => c,
        Err(error) => {
            details.push(format!("{error:#}"));
            return Some(Check::new(
                "Reinstalling scheduler",
                "failed",
                true,
                details,
            ));
        }
    };
    Some(
        match cron::install(repo_root, &config.sync.schedule, &current_exe) {
            Ok(_) => Check::new("Reinstalling scheduler", "applied", false, details),
            Err(error) => {
                details.push(format!("{error:#}"));
                Check::new("Reinstalling scheduler", "failed", true, details)
            }
        },
    )
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
