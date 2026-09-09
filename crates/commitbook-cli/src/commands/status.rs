use anyhow::Result;
use std::path::Path;

use commitbook_engine::cron;
use commitbook_engine::git::GitRepo;

pub fn run(_cb_dir: &Path, repo_root: &Path, json: bool) -> Result<()> {
    let status = commitbook_engine::inspection::RepositoryStatus::read(repo_root);
    if json {
        let mut obj = serde_json::to_value(&status)?;
        obj["has_remote"] =
            serde_json::json!(GitRepo::open(repo_root).is_ok_and(|r| r.has_remote()));
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("CommitBook Status");
        println!(
            "  Scheduler: {}",
            if cron::is_loaded(repo_root) {
                "running"
            } else {
                "stopped"
            }
        );
        for line in status.lines() {
            println!("  {line}");
        }
    }

    Ok(())
}
