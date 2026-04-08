pub mod global;
pub mod local;

pub use global::GlobalConfig;
pub use local::LocalConfig;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolve a repo path from a CLI flag or fall back to the current directory.
pub fn resolve_repo_path(flag_value: Option<&Path>) -> Result<PathBuf> {
    match flag_value {
        Some(p) => Ok(std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())),
        None => std::env::current_dir()
            .context("Cannot determine current directory"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_repo_path_with_value() {
        let result = resolve_repo_path(Some(Path::new("/tmp"))).unwrap();
        assert!(result.to_string_lossy().contains("tmp"));
    }

    #[test]
    fn test_resolve_repo_path_none_uses_cwd() {
        let result = resolve_repo_path(None).unwrap();
        assert_eq!(result, std::env::current_dir().unwrap());
    }
}
