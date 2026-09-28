use super::*;
use anyhow::anyhow;

#[test]
fn test_humanize_network() {
    let e = anyhow!("Could not resolve host: github.com").context("git fetch failed");
    let msg = humanize(&e);
    assert!(msg.contains("network"), "unexpected: {msg}");
}

#[test]
fn test_humanize_non_ff() {
    let e = anyhow!("non-fast-forward").context("git push failed");
    let msg = humanize(&e);
    assert!(msg.contains("Remote has diverged"), "unexpected: {msg}");
}

#[test]
fn test_humanize_auth() {
    let e = anyhow!("authentication required");
    let msg = humanize(&e);
    assert!(msg.contains("Authentication"), "unexpected: {msg}");
    assert!(msg.contains("Git credentials"), "unexpected: {msg}");
    assert!(!msg.contains("commitbook token"), "unexpected: {msg}");
}

#[test]
fn test_humanize_not_initialized() {
    let e = anyhow!("CommitBook is not initialized. Run `commitbook init` first.");
    let msg = humanize(&e);
    assert!(msg.contains("commitbook init"), "unexpected: {msg}");
}

#[test]
fn test_humanize_falls_back_to_top_context() {
    let e = anyhow!("low-level libgit2 error 0x42").context("Could not push origin/main");
    let msg = humanize(&e);
    // Top context wins: the libgit2 internals do not appear.
    assert!(msg.contains("Could not push"), "unexpected: {msg}");
    assert!(!msg.contains("libgit2"), "leaked internals: {msg}");
}

#[test]
fn verbose_hint_only_when_it_adds_detail() {
    let single = anyhow!("Cannot sync while checked out on branch \"x\"");
    assert!(!verbose_adds_detail(&single));
    let chained = anyhow!("low-level libgit2 error 0x42").context("Could not push origin/main");
    assert!(verbose_adds_detail(&chained));
}

#[test]
fn reported_error_displays_the_full_chain() {
    let inner = anyhow!("low-level libgit2 error 0x42").context("Could not push origin/main");
    let reported: anyhow::Error = Reported(inner).into();
    assert!(reported.downcast_ref::<Reported>().is_some());
    assert_eq!(
        reported.to_string(),
        "Could not push origin/main: low-level libgit2 error 0x42"
    );
}
