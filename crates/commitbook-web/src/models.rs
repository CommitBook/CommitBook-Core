use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub running: bool,
    pub enabled: bool,
    pub schedule: String,
    pub schedule_desc: String,
    pub branch: String,
    pub current_branch: String,
    pub auto_push: bool,
    pub last_commit: Option<String>,
    pub changes_total: usize,
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
    pub ai_messages: Option<bool>,
    pub schedule: Option<String>,
    pub auto_push: Option<bool>,
    pub branch: Option<String>,
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
