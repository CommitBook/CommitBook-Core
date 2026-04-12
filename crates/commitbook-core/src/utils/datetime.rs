use chrono::{Local, Utc};

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
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Format a duration as a human-readable relative time.
pub fn format_relative(seconds: i64) -> String {
    if seconds < 60 {
        format!("{}s ago", seconds)
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        if m > 0 {
            format!("{}h {}m ago", h, m)
        } else {
            format!("{}h ago", h)
        }
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

#[cfg(test)]
#[path = "datetime_tests.rs"]
mod tests;
