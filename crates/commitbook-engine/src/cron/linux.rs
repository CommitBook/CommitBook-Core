use anyhow::{Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CRON_COMMENT_PREFIX: &str = "# CommitBook: ";

/// Quote `value` as one POSIX shell word. Single quotes keep `$`, backticks
/// and `"` literal; an embedded `'` is closed, escaped, and reopened.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Read one quoted word at the start of `input`: a single-quoted word as
/// written by `shell_quote`, or the double-quoted form older releases wrote.
/// Returns the value and the rest of the input.
fn shell_unquote(input: &str) -> Option<(String, &str)> {
    if let Some(rest) = input.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_string(), &rest[end + 1..]));
    }
    let mut rest = input.strip_prefix('\'')?;
    let mut value = String::new();
    loop {
        let end = rest.find('\'')?;
        value.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        match rest.strip_prefix(r"\''") {
            Some(after) => {
                value.push('\'');
                rest = after;
            }
            None => return Some((value, rest)),
        }
    }
}

/// A path cron can carry in a command. `%` means "newline" to cron and a
/// line break would split the entry, and neither can be quoted reliably
/// across cron implementations.
fn cron_safe(label: &str, path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .with_context(|| format!("{label} path is not valid UTF-8: {}", path.display()))?;
    if text.contains('%') || text.chars().any(char::is_control) {
        anyhow::bail!(
            "{label} path cannot be scheduled with crontab because it contains `%` or a control character: {text}"
        );
    }
    Ok(text.to_string())
}

/// Tools a scheduled sync may start. Their directories go into the job's
/// PATH, because cron runs jobs with a minimal PATH (often `/usr/bin:/bin`).
const SCHEDULED_TOOLS: &[&str] = &["git", "claude", "codex", "gh", "gemini", "cursor-agent"];

/// Directories always on the job's PATH, so a tool installed later in a
/// standard place is still found.
const BASELINE_PATHS: &[&str] = &["/usr/local/bin", "/usr/bin", "/bin"];

/// Environment for the scheduled job, captured when it is installed. cron
/// starts jobs without the login session's PATH or ssh-agent, so without
/// these a scheduled sync cannot find the AI CLIs or push over SSH.
/// Values cron cannot carry (`%`, control characters) are left out.
pub(super) fn scheduled_environment() -> Vec<(&'static str, String)> {
    let home = dirs::home_dir();
    let mut dirs: Vec<String> = SCHEDULED_TOOLS
        .iter()
        .filter_map(|tool| which::which(tool).ok())
        .filter_map(|path| path.parent().map(|dir| dir.to_string_lossy().into_owned()))
        .collect();
    if let Some(home) = &home {
        for sub in [".local/bin", ".cargo/bin"] {
            dirs.push(home.join(sub).to_string_lossy().into_owned());
        }
    }
    dirs.extend(BASELINE_PATHS.iter().map(|dir| dir.to_string()));
    let mut seen = std::collections::HashSet::new();
    let path = dirs
        .into_iter()
        .filter(|dir| is_cron_safe(dir) && !dir.contains(':'))
        .filter(|dir| seen.insert(dir.clone()))
        .collect::<Vec<_>>()
        .join(":");

    let mut environment = vec![("PATH", path)];
    if let Ok(socket) = std::env::var("SSH_AUTH_SOCK") {
        if !socket.is_empty() && is_cron_safe(&socket) {
            environment.push(("SSH_AUTH_SOCK", socket));
        }
    }
    environment
}

fn is_cron_safe(text: &str) -> bool {
    !text.contains('%') && !text.chars().any(char::is_control)
}

/// Build the comment and crontab entry lines for a repo. `environment` is
/// set for the `commitbook` process only, as shell assignments in front of
/// it, so the user's other cron jobs are unaffected.
pub(super) fn build_crontab_entry(
    repo_path: &Path,
    schedule: &str,
    commitbook_bin: &Path,
    environment: &[(&str, String)],
) -> Result<(String, String)> {
    let repo = cron_safe("Repository", repo_path)?;
    let bin = cron_safe("CommitBook binary", commitbook_bin)?;
    let comment = format!("{CRON_COMMENT_PREFIX}{repo}");
    let assignments: String = environment
        .iter()
        .filter(|(_, value)| is_cron_safe(value))
        .map(|(name, value)| format!("{name}={} ", shell_quote(value)))
        .collect();
    let entry = format!(
        "{schedule} cd {} && {assignments}{} sync",
        shell_quote(&repo),
        shell_quote(&bin)
    );
    Ok((comment, entry))
}

/// Skip `NAME='value' ` shell assignments in front of the command.
fn skip_assignments(mut rest: &str) -> Option<&str> {
    loop {
        let Some(eq) = rest.find('=') else {
            return Some(rest);
        };
        let name = &rest[..eq];
        let is_name = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !is_name {
            return Some(rest);
        }
        let (_, after) = shell_unquote(&rest[eq + 1..])?;
        rest = after.strip_prefix(' ')?;
    }
}

/// The repository and binary of a crontab line CommitBook wrote:
/// `<schedule> cd <repo> && <bin> sync` (`run` in older releases). Any other
/// line, including a user's own job that mentions the repository, is `None`.
fn parse_entry(line: &str) -> Option<(String, String)> {
    let start = line.find(" cd ")? + " cd ".len();
    let (repo, rest) = shell_unquote(&line[start..])?;
    let rest = skip_assignments(rest.strip_prefix(" && ")?)?;
    let (bin, rest) = shell_unquote(rest)?;
    matches!(rest.trim_end(), " sync" | " run").then_some((repo, bin))
}

/// Remove this repo's CommitBook entries, and each marker comment directly
/// above one. Every other line, including comments and jobs that merely
/// mention the repository, is kept.
pub(super) fn filter_crontab_lines(current: &str, repo_path: &Path) -> String {
    let repo = repo_path.to_string_lossy();
    let marker = format!("{CRON_COMMENT_PREFIX}{repo}");
    let lines: Vec<&str> = current.lines().collect();
    let is_ours = |line: &str| parse_entry(line).is_some_and(|(entry_repo, _)| entry_repo == repo);
    let mut kept = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        if is_ours(line) {
            continue;
        }
        let marks_our_entry =
            line.trim() == marker && lines.get(index + 1).is_some_and(|next| is_ours(next));
        if marks_our_entry {
            continue;
        }
        kept.push(*line);
    }
    kept.join("\n")
}

/// Install the crontab entry for the repo, replacing an earlier one. The
/// crontab is read once and written once.
pub fn install(repo_path: &Path, schedule: &str, commitbook_bin: &Path) -> Result<()> {
    let (comment, entry) = build_crontab_entry(
        repo_path,
        schedule,
        commitbook_bin,
        &scheduled_environment(),
    )?;
    let current = get_current_crontab()?;
    let kept = filter_crontab_lines(&current, repo_path);
    let new_crontab = if kept.trim().is_empty() {
        format!("{comment}\n{entry}\n")
    } else {
        format!("{}\n{comment}\n{entry}\n", kept.trim_end())
    };
    set_crontab(&new_crontab)
}

/// Remove the crontab entry for the repo. The rest of the crontab is written
/// back as it was; the crontab is never deleted.
pub fn uninstall(repo_path: &Path) -> Result<()> {
    let current = get_current_crontab()?;
    let kept = filter_crontab_lines(&current, repo_path);
    if kept == current.trim_end_matches('\n') {
        return Ok(());
    }
    let content = if kept.is_empty() {
        String::new()
    } else {
        format!("{kept}\n")
    };
    set_crontab(&content)
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
    get_current_crontab()
        .map(|crontab| crontab_binary(&crontab, repo_path).is_some())
        .unwrap_or(false)
}

/// Binary path the repo's crontab entry launches.
pub fn scheduled_binary(repo_path: &Path) -> Option<PathBuf> {
    crontab_binary(&get_current_crontab().ok()?, repo_path)
}

/// The binary of this repo's CommitBook entry, if the crontab has one.
pub(super) fn crontab_binary(crontab: &str, repo_path: &Path) -> Option<PathBuf> {
    let repo = repo_path.to_string_lossy();
    crontab
        .lines()
        .filter_map(parse_entry)
        .find(|(entry_repo, _)| *entry_repo == repo)
        .map(|(_, bin)| PathBuf::from(bin))
}

/// The user's crontab, or empty when they have none. Any other failure is
/// an error: treating it as empty would make the next write replace the
/// user's jobs with CommitBook's alone.
fn get_current_crontab() -> Result<String> {
    let output = Command::new("crontab")
        .args(["-l"])
        .output()
        .context("Failed to read crontab")?;
    read_crontab_output(&output)
}

pub(super) fn read_crontab_output(output: &std::process::Output) -> Result<String> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.to_ascii_lowercase().contains("no crontab") {
        return Ok(String::new());
    }
    anyhow::bail!("Failed to read crontab: {}", stderr.trim())
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
