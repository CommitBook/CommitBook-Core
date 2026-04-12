use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const CRON_COMMENT_PREFIX: &str = "# CommitBook: ";

/// Build the comment and crontab entry lines for a repo.
pub(super) fn build_crontab_entry(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
) -> (String, String) {
    let repo_str = repo_path.to_string_lossy();
    let bin_str = commitbook_bin.to_string_lossy();
    let comment = format!("{}{}", CRON_COMMENT_PREFIX, repo_str);
    let entry = format!("{} \"{}\" auto-commit --repo \"{}\"", schedule, bin_str, repo_str);
    (comment, entry)
}

/// Filter out crontab lines belonging to the given repo.
pub(super) fn filter_crontab_lines(current: &str, repo_path: &Path) -> String {
    let repo_str = repo_path.to_string_lossy();
    let marker = format!("{}{}", CRON_COMMENT_PREFIX, repo_str);
    let repo_arg = format!("--repo \"{}\"", repo_str);
    let lines: Vec<&str> = current.lines().collect();
    let mut new_lines = Vec::new();
    let mut skip_next = false;

    for line in lines {
        if skip_next {
            skip_next = false;
            continue;
        }
        if line.trim() == marker {
            skip_next = true;
            continue;
        }
        if line.contains(&repo_arg) && line.contains("commitbook") {
            continue;
        }
        new_lines.push(line);
    }

    new_lines.join("\n")
}

/// Install a crontab entry for the repo. Returns "crontab:<repo_path>" as scheduler_id.
pub fn install(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
) -> Result<String> {
    // Remove any existing entry first
    let _ = uninstall(repo_path);

    let current = get_current_crontab().unwrap_or_default();
    let (comment, entry) = build_crontab_entry(repo_path, schedule, commitbook_bin);

    let new_crontab = if current.is_empty() {
        format!("{}\n{}\n", comment, entry)
    } else {
        format!("{}\n{}\n{}\n", current.trim_end(), comment, entry)
    };

    set_crontab(&new_crontab)?;

    Ok(format!("crontab:{}", repo_path.to_string_lossy()))
}

/// Remove the crontab entry for the repo.
pub fn uninstall(repo_path: &Path) -> Result<()> {
    let current = get_current_crontab().unwrap_or_default();

    if current.is_empty() {
        return Ok(());
    }

    let new_crontab = filter_crontab_lines(&current, repo_path);
    if new_crontab.trim().is_empty() {
        let _ = Command::new("crontab").arg("-r").output();
    } else {
        set_crontab(&format!("{}\n", new_crontab))?;
    }

    Ok(())
}

/// Check if crontab is accessible.
pub fn is_accessible() -> bool {
    Command::new("crontab")
        .args(["-l"])
        .output()
        .map(|o| o.status.success() || o.status.code() == Some(1))
        .unwrap_or(false)
}

/// Check if a crontab entry is loaded for the given repo.
pub fn is_loaded(repo_path: &Path) -> bool {
    let repo_str = repo_path.to_string_lossy();
    let repo_arg = format!("--repo \"{}\"", repo_str);
    get_current_crontab()
        .map(|crontab| {
            crontab.lines().any(|line| {
                line.contains(&repo_arg) && line.contains("commitbook")
            })
        })
        .unwrap_or(false)
}

fn get_current_crontab() -> Result<String> {
    let output = Command::new("crontab")
        .args(["-l"])
        .output()
        .context("Failed to read crontab")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Ok(String::new())
    }
}

fn set_crontab(content: &str) -> Result<()> {
    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(Stdio::piped())
        .spawn()
        .context("Failed to start crontab")?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(content.as_bytes())
            .context("Failed to write crontab")?;
    }

    let status = child.wait().context("Failed to wait for crontab")?;
    if !status.success() {
        anyhow::bail!("crontab command failed");
    }

    Ok(())
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
