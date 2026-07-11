use super::*;

#[test]
fn split_no_markers() {
    let (l, r) = split_conflict_markers("hello\nworld\n");
    assert_eq!(l, "hello\nworld\n");
    assert_eq!(r, "hello\nworld\n");
}

#[test]
fn split_simple_conflict() {
    let raw = "intro\n<<<<<<< HEAD\nlocal line\n=======\nremote line\n>>>>>>> origin/main\noutro\n";
    let (l, r) = split_conflict_markers(raw);
    assert_eq!(l, "intro\nlocal line\noutro\n");
    assert_eq!(r, "intro\nremote line\noutro\n");
}

#[test]
fn split_diff3_conflict_drops_ancestor() {
    // diff3/zdiff3 style: a `||||||| base` section sits between the local
    // side and the divider. It must not leak into either resolved side.
    let raw = "intro\n<<<<<<< HEAD\nlocal line\n||||||| base\nancestor line\n=======\nremote line\n>>>>>>> origin/main\noutro\n";
    let (l, r) = split_conflict_markers(raw);
    assert_eq!(l, "intro\nlocal line\noutro\n");
    assert_eq!(r, "intro\nremote line\noutro\n");
}
