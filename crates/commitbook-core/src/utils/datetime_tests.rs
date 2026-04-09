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
