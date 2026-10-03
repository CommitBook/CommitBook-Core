# Verification

## 1.2.0 preparation

The code candidate at `2729161` passed all 14 hosted PR checks:
[S1 CI](https://github.com/CommitBook/CommitBook-Core/actions/runs/37147527145)
and [Security Audit](https://github.com/CommitBook/CommitBook-Core/actions/runs/37147527199).
This includes the `cbook` alias, FFI crate/interface renames, older-release
installer compatibility, and locked SDK document reads. Publication still
requires checks and artifacts from the exact merged release commit; these
candidate results do not constitute published-release verification.

Local preparation checks at `2729161` with the release documentation changes
passed on 2026-10-03: 640 Rust tests, 11 release-tool tests, formatting,
warning-free Clippy, Rust 1.91 compatibility, workflow and shell syntax checks,
and dependency audit. The pinned redacted secret scanner found no leaks across
1,762 available commits. That result is bounded by the scanned refs and rules.

All desktop archives must contain five executables (`commitbook`, `cobo`,
`cbook`, `commitbook-tui`, `commitbook-web`) and both license files. The Apple
release must include the XCFramework ZIP and its checksum, with regenerated
bindings for `CommitBookEngineFfi` and a passing Swift smoke test.

The maintainer explicitly accepts real-iOS authenticated Git as unverified for
1.2.0. The framework smoke test is required but is not a substitute for that
real-app test. Android packaging and crates.io remain outside this release.

## Hosted releases

Checked against the hosted workflow runs, the release assets, and the
`CommitBook/homebrew-tap` history.

| Check | 1.0.0 (2026-09-28) | 1.0.1 (2026-09-29) |
|---|---|---|
| PR required checks | PR #7: 14/14 passed | PR #8: 14/14 passed |
| S2 release | Run 36485962626: 21/21 jobs passed; 4 archives, `SHA256SUMS`, `install.sh` | Run 36503475095: 21/21 jobs passed; same assets |
| S3 Homebrew | Run 36487809537: verify (style, audit, install, test on macOS) and publish passed; tap `fb3ca6c` | Run 36505709514: same jobs passed; tap `725ba9f` |
| S5 XCFramework | Not run | Run 36505751756: passed; ZIP and `.sha256` attached |
| Formula checksums | All 4 equal `SHA256SUMS` | All 4 equal `SHA256SUMS` |
| Local install | `brew install` on macOS arm64 gave `commitbook 1.0.0` | Homebrew on macOS arm64 reports `commitbook 1.0.1` |

## Pre-launch local check

Locally rechecked against branch `C/Homebrew` at `85fa005` on 2026-09-28,
before the first release. This code uses an eight-hex-character, OS-random
`commitbook_local_id` in the ignored `.CommitBook/local/commitbook_local_id.toml`.
SDK-created clones use that ID as their directory name and check for local
collisions. The older `CommitBook-ID.toml` is rejected rather than migrated.

| Check | Result at `85fa005` |
|---|---|
| Workspace Rust tests (`--locked`) | Passed: 617 tests |
| Warning-free workspace Clippy | Passed |
| Rust 1.91 workspace/all-targets check | Passed |
| Formatting and whitespace | Passed |
| Offline release/installer/Homebrew tool tests | Passed: 10 tests |
| Workflow validation (`actionlint`) | Passed |
| Dependency audit (`cargo audit --deny warnings`) | Passed; no findings |
| Pinned, redacted full-history secret scan | Passed: 1,428 commits across available refs; no findings |
| XCFramework release build and Swift smoke test | Passed: five slices; registration, ID readback, and async boundary; ZIP checksum, Swift source, and license contents verified |
| iOS device Clippy and simulator check | Passed |
| Native macOS arm64 desktop package | Passed: four binaries and two license files in the archive; both CLI names report version 0.10.0 |

## Remaining release gates

Only macOS arm64 archives have been installed and run from a published
release. S3 installs and tests the formula on macOS; the Linux archives get
only checksum and layout checks.

The Swift smoke test constructs a credential callback, but does not perform an
authenticated Git operation through the real iOS app. For 1.2.0 this is an
accepted, documented limitation rather than a publication gate. Validate that
flow with a disposable remote before claiming real-app authenticated Git
coverage; see the [1.2.0 release decision](release-readiness.md#120-release-decision).

The repository is public. Private vulnerability reporting, secret scanning,
push protection, `main` protection, and release-environment ref restrictions
are enabled. Follow
[release readiness](release-readiness.md) before tagging or announcing a public
release. A clean scanner result is bounded by the scanned refs and detector
rules, not proof that no secret exists.

The main README is [docs/README.md](README.md); no root README is retained.
