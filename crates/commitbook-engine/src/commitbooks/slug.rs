/// Convert an `(owner, repo)` pair into the deterministic on-disk slug.
///
/// Format: `<owner>__<repo>`. GitHub usernames allow only alphanumerics and
/// hyphens (no underscores), so the first `__` always marks the owner/repo
/// boundary even though repo names may contain underscores; splitting on the
/// first `__` round-trips cleanly. The same CommitBook ends up at the same
/// path on every device, so a fresh install can predict where a discovered
/// CommitBook would clone to.
pub fn slug_for(owner: &str, repo: &str) -> String {
    format!("{owner}__{repo}")
}

/// Inverse of `slug_for`. Returns `None` if the slug isn't well-formed.
pub fn parse_slug(slug: &str) -> Option<(String, String)> {
    let mut parts = slug.splitn(2, "__");
    let owner = parts.next()?.to_string();
    let repo = parts.next()?.to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner, repo))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_round_trips() {
        let s = slug_for("manuel", "notes");
        assert_eq!(s, "manuel__notes");
        let (o, r) = parse_slug(&s).unwrap();
        assert_eq!(o, "manuel");
        assert_eq!(r, "notes");
    }

    #[test]
    fn slug_handles_hyphenated_names() {
        let s = slug_for("acme-corp", "my-notes");
        let (o, r) = parse_slug(&s).unwrap();
        assert_eq!(o, "acme-corp");
        assert_eq!(r, "my-notes");
    }

    #[test]
    fn slug_round_trips_repo_with_underscores() {
        // Repo names may contain underscores (even doubled); owners cannot,
        // so the first `__` still splits unambiguously.
        let s = slug_for("acme", "my__notes_v2");
        let (o, r) = parse_slug(&s).unwrap();
        assert_eq!(o, "acme");
        assert_eq!(r, "my__notes_v2");
    }

    #[test]
    fn parse_slug_rejects_malformed() {
        assert!(parse_slug("noseparator").is_none());
        assert!(parse_slug("__only-repo").is_none());
        assert!(parse_slug("only-owner__").is_none());
    }
}
