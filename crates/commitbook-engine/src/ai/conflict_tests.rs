use super::*;

#[test]
fn manual_key_returns_none() {
    let registry = ResolverRegistry::new();
    assert!(registry.get("manual").is_none());
}

#[test]
fn unknown_key_returns_none() {
    let registry = ResolverRegistry::new();
    assert!(registry.get("nonexistent-tool").is_none());
}

#[test]
fn check_availability_lists_all_known_resolvers() {
    let registry = ResolverRegistry::new();
    let entries = registry.check_availability();
    let keys: Vec<&str> = entries.iter().map(|(k, _, _)| k.as_str()).collect();
    // On desktop builds all five should be present.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        for expected in &["claude", "codex", "copilot", "gemini", "cursor"] {
            assert!(keys.contains(expected), "missing resolver key: {expected}");
        }
    }
    // On mobile, the registry is empty.
    #[cfg(any(target_os = "ios", target_os = "android"))]
    assert!(entries.is_empty());
}

#[test]
fn strip_outer_code_fence_drops_simple_fence() {
    let raw = "```\nhello world\n```";
    assert_eq!(strip_outer_code_fence(raw), "hello world");
}

#[test]
fn strip_outer_code_fence_drops_language_tagged_fence() {
    let raw = "```markdown\n# Title\nbody\n```";
    assert_eq!(strip_outer_code_fence(raw), "# Title\nbody");
}

#[test]
fn strip_outer_code_fence_passes_through_when_no_fence() {
    let raw = "no fences here";
    assert_eq!(strip_outer_code_fence(raw), "no fences here");
}

#[test]
fn strip_outer_code_fence_preserves_nested_fences() {
    // An outer ```markdown wrapper around content that itself contains a
    // ```rust block should leave the inner block intact.
    let raw = "```markdown\nbefore\n```rust\nfn x() {}\n```\nafter\n```";
    assert_eq!(
        strip_outer_code_fence(raw),
        "before\n```rust\nfn x() {}\n```\nafter"
    );
}

#[test]
fn strip_outer_code_fence_keeps_note_opening_with_fence_then_prose() {
    // A note that opens with a fenced code block and then continues with prose
    // must not be truncated at the inner closing fence.
    let raw = "```python\ncode\n```\nprose";
    assert_eq!(strip_outer_code_fence(raw), "```python\ncode\n```\nprose");
}

#[test]
fn build_resolve_prompt_includes_path_and_structured_sides() {
    let side = |content: &str| crate::git::ConflictSide {
        oid: git2::Oid::ZERO_SHA1,
        mode: 0o100644,
        content: content.as_bytes().to_vec(),
    };
    let conflict = crate::git::GitConflict {
        path: "notes/intro.md".to_string(),
        ancestor: Some(side("base")),
        local: Some(side("local")),
        remote: Some(side("remote")),
    };
    let prompt = build_resolve_prompt(&conflict).unwrap();
    assert!(prompt.contains("notes/intro.md"));
    assert!(prompt.contains("ANCESTOR:\nbase"));
    assert!(prompt.contains("LOCAL:\nlocal"));
    assert!(prompt.contains("REMOTE:\nremote"));
    assert!(!prompt.contains("<<<<<<<"));
}

#[test]
fn finalize_resolved_text_accepts_setext_heading_underline() {
    let raw = "Title\n=======\n\nbody text";
    let resolved = finalize_resolved_text(raw, "test CLI").unwrap();
    assert_eq!(resolved, ConflictResolution::WriteContent(raw.to_string()));
}

#[test]
fn finalize_resolved_text_strips_outer_fence() {
    let raw = "```markdown\n# Title\nbody\n```";
    let resolved = finalize_resolved_text(raw, "test CLI").unwrap();
    assert_eq!(
        resolved,
        ConflictResolution::WriteContent("# Title\nbody".to_string())
    );
}

#[test]
fn finalize_resolved_text_rejects_real_conflict_markers() {
    let raw = "intro\n<<<<<<< ours\nleft\n=======\nright\n>>>>>>> theirs\n";
    let err = finalize_resolved_text(raw, "test CLI").unwrap_err();
    assert!(err.to_string().contains("left conflict markers"));
    assert!(err.to_string().contains("test CLI"));
}

#[test]
fn finalize_resolved_text_rejects_empty_output() {
    let err = finalize_resolved_text("  \n```\n\n```\n", "test CLI").unwrap_err();
    assert!(err.to_string().contains("Empty resolution from test CLI"));
}
