use chrono::Local;

/// Returns the current timestamp formatted for commit messages.
/// Example: "2026-02-15 14:30:02"
pub fn now_formatted() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Returns the current date for log file naming.
/// Example: "2026-02-15"
pub fn today_date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// Returns the current timestamp in ISO 8601 format for config storage.
pub fn now_iso() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
