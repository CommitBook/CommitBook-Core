use super::*;
use std::time::{Duration, Instant};

fn sh(script: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(script);
    command
}

#[cfg(unix)]
#[test]
fn drains_large_output_without_deadlock() {
    // Far past the ~64 KB pipe buffer that would deadlock a waiter that reads
    // the pipe only after the child exits.
    let output = run_bounded(
        &mut sh("yes 0123456789ABCDEF | head -c 200000"),
        None,
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 200_000);
}

#[cfg(unix)]
#[test]
fn without_input_the_child_sees_end_of_stdin() {
    let output = run_bounded(&mut sh("cat; echo done"), None, Duration::from_secs(30)).unwrap();
    assert_eq!(output.stdout, b"done\n");
}

#[cfg(unix)]
#[test]
fn a_grandchild_holding_stdout_cannot_outlast_the_deadline() {
    // The child exits at once, but its background job keeps stdout open.
    // Waiting for EOF alone would block for the full minute.
    let started = Instant::now();
    let result = run_bounded(
        &mut sh("sleep 60 & echo started"),
        None,
        Duration::from_millis(300),
    );
    assert!(started.elapsed() < Duration::from_secs(10), "{result:?}");
    let error = result.unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error:#}");
}

#[cfg(unix)]
#[test]
fn a_child_that_never_exits_is_killed_at_the_deadline() {
    let started = Instant::now();
    let error = run_bounded(
        &mut sh("sleep 60"),
        Some(b"input"),
        Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error:#}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_missing_program_is_reported() {
    let error = run_bounded(
        &mut Command::new("commitbook-no-such-program"),
        None,
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("commitbook-no-such-program"),
        "{error:#}"
    );
}

#[cfg(unix)]
#[test]
fn reports_whether_the_child_read_all_input() {
    let input = vec![b'x'; 1024 * 1024];
    let partial = run_bounded_tracking_input(
        &mut sh("echo early; exit 0"),
        Some(&input),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(partial.output.status.success());
    assert!(!partial.input_complete);

    let full = run_bounded_tracking_input(
        &mut sh("cat >/dev/null"),
        Some(&input),
        Duration::from_secs(30),
    )
    .unwrap();
    assert!(full.input_complete);
}
