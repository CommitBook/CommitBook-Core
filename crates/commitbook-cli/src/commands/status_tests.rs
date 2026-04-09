use super::*;

#[test]
fn test_format_until_zero() {
    assert_eq!(format_until(0), "0s");
}

#[test]
fn test_format_until_seconds() {
    assert_eq!(format_until(45), "45s");
}

#[test]
fn test_format_until_boundary_59() {
    assert_eq!(format_until(59), "59s");
}

#[test]
fn test_format_until_boundary_60() {
    assert_eq!(format_until(60), "1m");
}

#[test]
fn test_format_until_minutes_truncates() {
    assert_eq!(format_until(150), "2m");
}

#[test]
fn test_format_until_boundary_3599() {
    assert_eq!(format_until(3599), "59m");
}

#[test]
fn test_format_until_boundary_3600() {
    assert_eq!(format_until(3600), "1h");
}

#[test]
fn test_format_until_hours_and_minutes() {
    assert_eq!(format_until(3660), "1h 1m");
}

#[test]
fn test_format_until_exact_hours() {
    assert_eq!(format_until(7200), "2h");
}
