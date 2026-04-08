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
mod tests {
    use super::*;

    #[test]
    fn test_now_formatted_pattern() {
        let s = now_formatted();
        assert_eq!(s.len(), 19);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], " ");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
    }

    #[test]
    fn test_today_date_pattern() {
        let s = today_date();
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
    }

    #[test]
    fn test_now_iso_pattern() {
        let s = now_iso();
        assert!(s.contains('T'));
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn test_format_relative_seconds() {
        assert_eq!(format_relative(45), "45s ago");
    }

    #[test]
    fn test_format_relative_minutes() {
        assert_eq!(format_relative(150), "2m ago");
    }

    #[test]
    fn test_format_relative_hours_and_minutes() {
        assert_eq!(format_relative(3660), "1h 1m ago");
    }

    #[test]
    fn test_format_relative_days() {
        assert_eq!(format_relative(172800), "2d ago");
    }
}
