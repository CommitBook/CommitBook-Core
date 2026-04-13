use anyhow::{Context, Result};
use std::path::Path;

/// Read the base version of a file (last successfully synced version).
pub fn read(commitbook_dir: &Path, file_path: &str) -> Result<Option<String>> {
    let path = commitbook_dir.join("local").join("base").join(file_path);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read base: {}", path.display()))?;
    Ok(Some(content))
}

/// Write the base version of a file.
pub fn write(commitbook_dir: &Path, file_path: &str, content: &str) -> Result<()> {
    let path = commitbook_dir.join("local").join("base").join(file_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write base: {}", path.display()))?;
    Ok(())
}

/// Delete the base version of a file.
pub fn delete(commitbook_dir: &Path, file_path: &str) -> Result<()> {
    let path = commitbook_dir.join("local").join("base").join(file_path);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// List all files in the base directory (relative paths).
pub fn list(commitbook_dir: &Path) -> Result<Vec<String>> {
    let base_dir = commitbook_dir.join("local").join("base");
    if !base_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    walk_dir(&base_dir, &base_dir, &mut files)?;
    Ok(files)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    files: &mut Vec<String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(root, &path, files)?;
        } else {
            let rel = path
                .strip_prefix(root)?
                .to_string_lossy()
                .to_string();
            files.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "base_tests.rs"]
mod tests;
