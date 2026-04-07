use anyhow::Result;
use colored::*;
use std::path::Path;
use std::process::Command;

use commitbook_core::ai::ProviderChain;
use commitbook_core::config::{GlobalConfig, LocalConfig};
use commitbook_core::cron;
use commitbook_core::git::GitRepo;

struct Check {
    name: String,
    passed: bool,
    detail: String,
    required: bool,
}

impl Check {
    fn pass(name: &str, detail: &str) -> Self {
        Self { name: name.to_string(), passed: true, detail: detail.to_string(), required: true }
    }
    fn fail(name: &str, detail: &str) -> Self {
        Self { name: name.to_string(), passed: false, detail: detail.to_string(), required: true }
    }
    fn optional_pass(name: &str, detail: &str) -> Self {
        Self { name: name.to_string(), passed: true, detail: detail.to_string(), required: false }
    }
    fn optional_fail(name: &str, detail: &str) -> Self {
        Self { name: name.to_string(), passed: false, detail: detail.to_string(), required: false }
    }

    fn display(&self) {
        let icon = if self.passed {
            "OK".green().bold()
        } else if self.required {
            "FAIL".red().bold()
        } else {
            "SKIP".yellow().bold()
        };
        let detail = if self.passed {
            self.detail.green().to_string()
        } else if self.required {
            self.detail.red().to_string()
        } else {
            self.detail.yellow().to_string()
        };
        println!("  [{}] {}: {}", icon, self.name, detail);
    }
}

pub fn run(repo_path: &Path) -> Result<()> {
    println!("{}", "CommitBook Doctor".bold().cyan());

    // Core checks
    println!();
    println!("  {}", "Core".bold());
    let core_checks = vec![
        check_git(),
        check_git_repo(repo_path),
        check_git_remote(repo_path),
        check_scheduler(),
    ];
    for c in &core_checks { c.display(); }

    // AI provider checks
    println!();
    println!("  {}", "AI Providers".bold());
    let global = GlobalConfig::load().unwrap_or_default();
    let chain = ProviderChain::new();
    let ai_checks: Vec<Check> = chain
        .check_availability(&global.ai.providers)
        .into_iter()
        .map(|(key, name, available)| {
            if available {
                Check::optional_pass(&name, "Available")
            } else {
                Check::optional_fail(&name, &format!("Not found ({})", key))
            }
        })
        .collect();
    for c in &ai_checks { c.display(); }

    // CommitBook checks
    println!();
    println!("  {}", "CommitBook".bold());
    let cb_checks = vec![
        check_commitbook_config(repo_path),
        check_logs_writable(repo_path),
        check_binary_path(repo_path),
    ];
    for c in &cb_checks { c.display(); }

    // Summary
    let all_checks: Vec<&Check> = core_checks.iter()
        .chain(ai_checks.iter())
        .chain(cb_checks.iter())
        .collect();
    let passed = all_checks.iter().filter(|c| c.passed).count();
    let required_failed = all_checks.iter().filter(|c| !c.passed && c.required).count();

    println!();
    if required_failed == 0 {
        println!("  {} All core checks passed! ({}/{} total)", "OK".green().bold(), passed, all_checks.len());
    } else {
        println!("  {} {} required check(s) failed.", "!!".red().bold(), required_failed);
    }

    Ok(())
}

fn check_git() -> Check {
    match Command::new("git").args(["--version"]).output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Check::pass("Git", &version)
        }
        _ => Check::fail("Git", "git is not installed"),
    }
}

fn check_git_repo(repo_path: &Path) -> Check {
    if GitRepo::is_repo(repo_path) {
        Check::pass("Git repository", &format!("{}", repo_path.display()))
    } else {
        Check::fail("Git repository", &format!("Not a git repo: {}", repo_path.display()))
    }
}

fn check_git_remote(repo_path: &Path) -> Check {
    if !GitRepo::is_repo(repo_path) {
        return Check::fail("Git remote", "Not a git repository");
    }
    match commitbook_core::git::remote::check_remote_connectivity(repo_path) {
        Ok(true) => {
            let url = commitbook_core::git::remote::get_remote_url(repo_path, "origin")
                .unwrap_or_else(|_| "unknown".to_string());
            Check::pass("Git remote", &url)
        }
        Ok(false) => Check::optional_fail("Git remote", "Remote is not reachable"),
        Err(e) => Check::optional_fail("Git remote", &format!("Error: {}", e)),
    }
}

fn check_scheduler() -> Check {
    if cron::is_accessible() {
        let name = if cfg!(target_os = "macos") { "launchd" } else { "crontab" };
        Check::pass("Scheduler", &format!("{} is accessible", name))
    } else {
        Check::fail("Scheduler", "Scheduler is not accessible")
    }
}

fn check_commitbook_config(repo_path: &Path) -> Check {
    if !LocalConfig::exists(repo_path) {
        return Check::fail("CommitBook config", "Not set up (run: commitbook setup)");
    }
    match LocalConfig::load(repo_path) {
        Ok(config) => {
            let status = if config.enabled { "enabled" } else { "disabled" };
            let schedule = cron::describe_schedule(&config.schedule);
            Check::pass("CommitBook config", &format!("{}, schedule: {}", status, schedule))
        }
        Err(e) => Check::fail("CommitBook config", &format!("Corrupt: {}", e)),
    }
}

fn check_logs_writable(repo_path: &Path) -> Check {
    let logs_dir = LocalConfig::logs_dir(repo_path);
    if !logs_dir.exists() {
        if !LocalConfig::exists(repo_path) {
            return Check::optional_fail("Logs directory", "Not set up");
        }
        if std::fs::create_dir_all(&logs_dir).is_err() {
            return Check::fail("Logs directory", &format!("Cannot create: {}", logs_dir.display()));
        }
    }
    let test_file = logs_dir.join(".commitbook_test");
    match std::fs::write(&test_file, "test") {
        Ok(_) => {
            let _ = std::fs::remove_file(&test_file);
            Check::pass("Logs directory", "Writable")
        }
        Err(_) => Check::fail("Logs directory", &format!("Not writable: {}", logs_dir.display())),
    }
}

fn check_binary_path(repo_path: &Path) -> Check {
    #[cfg(target_os = "macos")]
    {
        match commitbook_core::cron::macos::validate_binary_path(repo_path) {
            Ok(true) => Check::pass("Binary path", "Plist binary exists"),
            Ok(false) => {
                if !LocalConfig::exists(repo_path) || !cron::is_loaded(repo_path) {
                    return Check::optional_fail("Binary path", "No active plist");
                }
                Check::fail("Binary path", "Binary in plist no longer exists — reinstall with `commitbook start`")
            }
            Err(_) => Check::optional_fail("Binary path", "Could not check"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = repo_path;
        Check::optional_pass("Binary path", "N/A (non-macOS)")
    }
}
