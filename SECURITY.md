# Security policy

CommitBook is pre-launch. Security fixes target the current development branch
and, once published, the latest release. Older development formats have no
compatibility or migration guarantee.

## Reporting a vulnerability

The intended reporting channel is GitHub private vulnerability reporting:
[Report a vulnerability](https://github.com/CommitBook/CommitBook-Core/security/advisories/new).
Do not include credentials, private CommitBook contents, or exploit details in a
public issue. Share a minimal reproduction using synthetic data through the
private reporting channel.

**Launch prerequisite:** maintainers must enable and test that channel before
making the repository public. It was not verifiable during this private-repo
preparation; this document does not claim that it is currently enabled. Do not
substitute a public issue if private reporting is unavailable.

## Trust boundaries

- Sync snapshots all tracked and non-ignored files, not only Markdown. Review
  `.gitignore` and use `commitbook preview` before initialization or syncing.
- `.CommitBook/config.toml` and `devices/` are shared through Git. Never place
  credentials there. Device-local state is under `.CommitBook/local/` and is
  explicitly excluded from staging as well as ignored by Git.
- Desktop transport normally uses system Git credentials. Optional file-backed
  tokens use `local/auth.toml` with restricted permissions. Native apps should
  keep tokens in platform secure storage and pass them to the SDK as needed.
- Timestamp commit messages do not invoke AI. Opting into AI messages or conflict
  resolution can send diffs or conflicting content to the selected CLI/provider.
  Review that provider's settings and data handling before enabling AI.
- The web dashboard is loopback-only and has no login. Host/origin checks protect
  against browser-driven cross-site requests; it is not intended for public
  hosting and does not isolate the CommitBook from other processes on the device.
- CommitBook-Ids are short local selectors, not secrets or authorization tokens.
  Filesystem containment and the host application's permissions remain required.
- Advisory locks coordinate CommitBook writers, not unrelated editors or Git
  commands. They are not a security boundary against a malicious local process.

## Maintenance

CI runs dependency and redacted secret scans. Release binaries bundle native
libraries, so dependency fixes require rebuilding and shipping artifacts.
Checksums detect mismatched downloads; they are not independent publisher
signatures. Keep license materials with distributed binaries. See the release
readiness checklist for required repository protections and unresolved gates.
