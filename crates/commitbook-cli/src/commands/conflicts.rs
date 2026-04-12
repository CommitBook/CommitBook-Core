use anyhow::Result;
use colored::Colorize;
use std::path::Path;

/// Show open conflicts by scanning for conflict markers in tracked files.
pub fn run(_cb_dir: &Path, repo_root: &Path) -> Result<()> {
    let mut conflict_files = Vec::new();

    scan_for_conflicts(repo_root, repo_root, &mut conflict_files)?;

    if conflict_files.is_empty() {
        println!("{}", "No conflicts found.".green());
        return Ok(());
    }

    println!(
        "{}",
        format!("{} file(s) with conflicts:", conflict_files.len())
            .yellow()
            .bold()
    );
    println!();

    for (path, count) in &conflict_files {
        println!(
            "  {} {} ({} conflict marker(s))",
            "CONFLICT".red().bold(),
            path,
            count
        );
    }

    println!();
    println!(
        "{}",
        "Resolve conflicts in the files above, then run `commitbook sync`.".dimmed()
    );

    Ok(())
}

/// Scan for conflict markers (<<<<<<< LOCAL) in markdown files.
fn scan_for_conflicts(
    root: &Path,
    dir: &Path,
    results: &mut Vec<(String, usize)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden dirs and .CommitBook/
        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            scan_for_conflicts(root, &path, results)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "markdown")
        {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let count = content
                    .lines()
                    .filter(|line| line.starts_with("<<<<<<< LOCAL"))
                    .count();
                if count > 0 {
                    let rel = path
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .to_string();
                    results.push((rel, count));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "conflicts_tests.rs"]
mod tests;
