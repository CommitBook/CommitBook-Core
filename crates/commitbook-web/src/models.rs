use serde::{Deserialize, Serialize};

fn stopped() -> commitbook_engine::cron::SchedulerHealth {
    commitbook_engine::cron::SchedulerHealth::Stopped
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    #[serde(default)]
    pub repository: commitbook_engine::inspection::RepositoryStatus,
    pub running: bool,
    #[serde(default = "stopped")]
    pub scheduler: commitbook_engine::cron::SchedulerHealth,
    #[serde(default)]
    pub scheduler_warning: Option<String>,
    pub schedule: String,
    pub schedule_desc: String,
    pub branch: String,
    pub current_branch: String,
    pub last_commit: Option<String>,
    pub changes_total: Option<usize>,
    pub changes_summary: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogsResponse {
    pub entries: Vec<LogEntry>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub level: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigUpdate {
    pub name: Option<String>,
    pub schedule: Option<String>,
    pub branch: Option<String>,
    pub commit_mode: Option<commitbook_engine::config::CommitMode>,
    pub commit_agent: Option<commitbook_engine::config::CommitAgent>,
    pub conflict_mode: Option<commitbook_engine::config::ConflictMode>,
    pub conflict_agent: Option<commitbook_engine::config::Agent>,
    pub log_keep: Option<commitbook_engine::config::LogKeep>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub key: String,
    pub name: String,
    pub available: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActionResponse {
    pub success: bool,
    pub message: String,
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
