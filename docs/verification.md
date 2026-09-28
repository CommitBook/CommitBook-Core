# Pre-launch verification

Locally rechecked against branch `C/Homebrew` at `85fa005` on 2026-09-28.
This code uses an eight-hex-character, OS-random `commitbook_local_id` in the
ignored `.CommitBook/local/commitbook_local_id.toml`. SDK-created clones use
that ID as their directory name and check for local collisions. The older
`CommitBook-ID.toml` is rejected rather than migrated.

## Results

| Check | Current result |
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
| iOS device Clippy and simulator check | Passed on current HEAD |
| Native macOS arm64 desktop package | Passed: four binaries and two license files in the archive; both CLI names report version 0.10.0 |

## Remaining release gates

Draft PR #6 has started hosted CI. Its first coverage job failed because the
report output directory did not exist; a workflow fix needs a fresh green run.
No GitHub release or tag has been published, so the
Homebrew formula-generation workflow has no real assets to consume. The private
`CommitBook/homebrew-tap` repository still contains no formula. Local tool tests
do not replace a live `brew install` and `brew test` against published assets.
The Swift smoke test constructs a credential callback, but does not perform an
authenticated Git operation through the real iOS app; validate that flow with
a disposable remote before release.

The repository is public. Private vulnerability reporting, secret scanning,
push protection, `main` protection, and release-environment ref restrictions
are enabled. Follow
[release readiness](release-readiness.md) before tagging or announcing a public
release. A clean scanner result is bounded by the scanned refs and detector
rules, not proof that no secret exists.

The main README is [docs/README.md](README.md); no root README is retained.
