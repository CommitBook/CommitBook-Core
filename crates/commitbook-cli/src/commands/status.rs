use anyhow::Result;
use std::path::Path;

use commitbook_engine::cron;
use commitbook_engine::git::GitRepo;

pub fn run(_cb_dir: &Path, repo_root: &Path, json: bool) -> Result<()> {
    let status = commitbook_engine::inspection::RepositoryStatus::read(repo_root);
    let scheduler = cron::health(repo_root);
    let warning = scheduler.warning(
        status.schedule.as_deref(),
        status.last_attempt_at.as_deref(),
        chrono::Utc::now(),
    );
    if json {
        let mut obj = serde_json::to_value(&status)?;
        obj["has_remote"] =
            serde_json::json!(GitRepo::open(repo_root).is_ok_and(|r| r.has_remote()));
        obj["scheduler"] = serde_json::to_value(&scheduler)?;
        obj["scheduler_warning"] = serde_json::json!(warning);
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("CommitBook Status");
        println!("  Scheduler: {}", scheduler.label());
        if let Some(warning) = &warning {
            println!("  {warning}");
        }
        for line in status.lines() {
            println!("  {line}");
        }
    }

    Ok(())
}
