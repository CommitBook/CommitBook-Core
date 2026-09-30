# Open-source release readiness

## Supported launch channels

| Surface | Launch status |
|---|---|
| CLI (`commitbook`, `cobo`, `cbook`), TUI, local web | macOS and Linux, arm64 and x86_64 artifacts |
| Homebrew | Published to `CommitBook/homebrew-tap` by S3 (1.0.0 onward) |
| Native SDK | Apple XCFramework, with Swift compile/link/runtime smoke test |
| Android | Bindings/integration work exists; no supported packaged release yet |
| crates.io | Deferred; use GitHub releases or documented source installation |

No visibility change, tag, merge, release publication, or credential rotation is
part of preparation. Pre-launch breaking API changes have no migration promise.

## Required code and artifact checks

- Formatting, warning-free Clippy, workspace tests, and Rust 1.91 compatibility.
- Linux/macOS CI and Apple target checks; XCFramework Swift smoke test.
- Locked dependency audit and full-history redacted secret scan.
- Matching workspace version, release tag, and exact checked/tested commit.
- All desktop executables including `cobo` and `cbook`; project and third-party license texts.
- Installer checksums/layout checks and release-tool fixture tests.

Use `cargo install cargo-about --version 0.9.2 --features cli --locked` for license
material generation. `scripts/release/notices.py` combines resolved Rust license
texts with bundled C/assembly-library notices. Review generator failures and
license changes; do not bypass them to publish. Build outputs are not committed.

## GitHub settings: external launch gates

The source workflows cannot enforce repository-level controls by themselves.
Before launch, maintainers must verify:

- `main` requires a pull request and successful CI/audit checks, blocks force
  pushes and deletion, and applies protections to administrators.
- The `release` environment restricts deployment to `main` and version-shaped
  tags.
- Private vulnerability reporting is enabled and its reporting link works.
- Secret scanning/push protection are enabled where available, in addition to
  the checked-in full-history scanner.
- Release tags (`*.*.*`) cannot be deleted or moved (ruleset "Protect release
  tags", with admin bypass).
- No unresolved dependency advisories, credential findings, or license gaps.

The repository is now public. Private vulnerability reporting, secret scanning,
push protection, branch protection, and release-environment ref restrictions
have been enabled. Hosted PR checks, the published 1.0.0 and 1.0.1 releases,
and a live Homebrew installation have passed. The authenticated Git flow
through the real iOS app is still unverified; no release so far has checked it.

See [workflow stages and Homebrew setup](workflows.md) for S1/S2/S3 dispatch
commands, required credentials, coverage artifacts, and publication checks.

## Recorded verification

See `docs/verification.md` for the execution results and remaining gates. Scanner
reports must be fully redacted; never commit credentials, raw secret matches,
local machine paths, or private CommitBook contents. A clean scanner result is
bounded by the scanned refs and detector rules, not proof that no secret exists.
