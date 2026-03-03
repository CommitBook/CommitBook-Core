use anyhow::Result;
use colored::*;
use std::path::Path;
use std::process::Command;

use crate::ai;
use crate::config::LocalConfig;
use crate::cron;
use crate::git::GitRepo;

struct CheckResult {
    name: String,
    passed: bool,
    detail: String,
}

impl CheckResult {
    fn pass(name: &str, detail: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            detail: detail.to_string(),
        }
    }

    fn fail(name: &str, detail: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            detail: detail.to_string(),
        }
    }

    fn display(&self) {
        let icon = if self.passed {
            "✓".green().bold()
        } else {
            "✗".red().bold()
        };
        let status = if self.passed {
            self.detail.green().to_string()
        } else {
            self.detail.red().to_string()
        };
        println!("  {} {}: {}", icon, self.name, status);
    }
}

/// Run the doctor command to check system health.
pub fn run(repo_path: &Path) -> Result<()> {
    println!("{}", "CommitBook Doctor".bold().cyan());
    println!();

    let mut results = Vec::new();

    // 1. Git installation
    results.push(check_git());

    // 2. Git repository
    results.push(check_git_repo(repo_path));

    // 3. Git remote connectivity
    results.push(check_git_remote(repo_path));

    // 4. GitHub CLI
    results.push(check_gh_cli());

    // 5. GitHub Copilot CLI
    results.push(check_copilot_cli());

    // 6. GitHub auth status
    results.push(check_gh_auth());

    // 7. CommitBook configuration
    results.push(check_commitbook_config(repo_path));

    // 8. Scheduler (cron/launchd)
    results.push(check_scheduler());

    // 9. Logs directory writable
    results.push(check_logs_writable(repo_path));

    // Display results
    for result in &results {
        result.display();
    }

    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();
    let failed = total - passed;

    println!();
    if failed == 0 {
        println!(
            "  {} All {} checks passed!",
            "✓".green().bold(),
            total
        );
    } else {
        println!(
            "  {} {}/{} checks passed, {} failed.",
            "!".yellow().bold(),
            passed,
            total,
            failed
        );
    }

    Ok(())
}

fn check_git() -> CheckResult {
    match Command::new("git").args(["--version"]).output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            CheckResult::pass("Git", &version)
        }
        _ => CheckResult::fail("Git", "git is not installed"),
    }
}

fn check_git_repo(repo_path: &Path) -> CheckResult {
    if GitRepo::is_repo(repo_path) {
        CheckResult::pass("Git repository", &format!("{}", repo_path.display()))
    } else {
        CheckResult::fail(
            "Git repository",
            &format!("Not a git repo: {}", repo_path.display()),
        )
    }
}

fn check_git_remote(repo_path: &Path) -> CheckResult {
    if !GitRepo::is_repo(repo_path) {
        return CheckResult::fail("Git remote", "Not a git repository");
    }

    match crate::git::remote::check_remote_connectivity(repo_path) {
        Ok(true) => {
            let url = crate::git::remote::get_remote_url(repo_path, "origin")
                .unwrap_or_else(|_| "unknown".to_string());
            CheckResult::pass("Git remote", &url)
        }
        Ok(false) => CheckResult::fail("Git remote", "Remote is not reachable"),
        Err(e) => CheckResult::fail("Git remote", &format!("Error: {}", e)),
    }
}

fn check_gh_cli() -> CheckResult {
    match which::which("gh") {
        Ok(path) => {
            let version = Command::new("gh")
                .args(["--version"])
                .output()
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| path.display().to_string());
            CheckResult::pass("GitHub CLI", &version)
        }
        Err(_) => CheckResult::fail("GitHub CLI", "gh is not installed"),
    }
}

fn check_copilot_cli() -> CheckResult {
    if ai::is_provider_available("gh-copilot") {
        CheckResult::pass("GitHub Copilot CLI", "Available")
    } else {
        CheckResult::fail(
            "GitHub Copilot CLI",
            "Not available (run: gh extension install github/gh-copilot)",
        )
    }
}

fn check_gh_auth() -> CheckResult {
    if crate::ai::copilot::is_authenticated() {
        CheckResult::pass("GitHub auth", "Authenticated")
    } else {
        CheckResult::fail(
            "GitHub auth",
            "Not authenticated (run: gh auth login)",
        )
    }
}

fn check_commitbook_config(repo_path: &Path) -> CheckResult {
    if !LocalConfig::exists(repo_path) {
        return CheckResult::fail(
            "CommitBook config",
            "Not set up (run: commitbook setup)",
        );
    }

    match LocalConfig::load(repo_path) {
        Ok(config) => {
            let status = if config.enabled { "enabled" } else { "disabled" };
            let schedule = cron::describe_schedule(&config.schedule);
            CheckResult::pass(
                "CommitBook config",
                &format!("{}, schedule: {}", status, schedule),
            )
        }
        Err(e) => CheckResult::fail("CommitBook config", &format!("Corrupt: {}", e)),
    }
}

fn check_scheduler() -> CheckResult {
    if cron::is_accessible() {
        let name = if cfg!(target_os = "macos") {
            "launchd"
        } else {
            "crontab"
        };
        CheckResult::pass("Scheduler", &format!("{} is accessible", name))
    } else {
        CheckResult::fail("Scheduler", "Scheduler is not accessible")
    }
}

fn check_logs_writable(repo_path: &Path) -> CheckResult {
    let logs_dir = LocalConfig::logs_dir(repo_path);

    if !logs_dir.exists() {
        if !LocalConfig::exists(repo_path) {
            return CheckResult::fail(
                "Logs directory",
                "Not set up (run: commitbook setup)",
            );
        }
        // Try to create it
        if std::fs::create_dir_all(&logs_dir).is_err() {
            return CheckResult::fail(
                "Logs directory",
                &format!("Cannot create: {}", logs_dir.display()),
            );
        }
    }

    // Check write permissions by creating a temp file
    let test_file = logs_dir.join(".commitbook_test");
    match std::fs::write(&test_file, "test") {
        Ok(_) => {
            let _ = std::fs::remove_file(&test_file);
            CheckResult::pass("Logs directory", &format!("{}", logs_dir.display()))
        }
        Err(_) => CheckResult::fail(
            "Logs directory",
            &format!("Not writable: {}", logs_dir.display()),
        ),
    }
}
