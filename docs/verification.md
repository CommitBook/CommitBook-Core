# Pre-launch verification

Verification first used an isolated feature snapshot and its baseline lockfile.
A separate session subsequently committed dependency/toolchain updates. The six
feature patches were rebased on that commit without incorporating its upgrades
into these feature commits; current-branch checks are recorded separately below.

## Results

| Check | Result |
|---|---|
| Workspace Rust tests | Passed: 612 tests on both the isolated snapshot and updated branch |
| Rust 1.91 workspace/all-targets check | Passed on the isolated snapshot and updated branch |
| Formatting and whitespace checks | Passed |
| Release/installer fixture tests | Passed: 3 tests, including piped installation |
| GitHub workflow validation (`actionlint`) | Passed |
| Dependency audit (`cargo audit --deny warnings`) | Passed on both lockfiles; no findings |
| Pinned redacted full-history scan | Passed: 1,343 commits across available refs; no findings |
| Redacted scan of proposed source changes | Passed; no findings |
| License material generation, including bundled native sources | Passed |
| Workspace Clippy | Passed on the isolated snapshot and updated branch |
| Apple device Clippy and simulator check | Passed on the isolated feature snapshot |
| XCFramework assembly and Swift runtime smoke test | Passed: five slices, Swift registration/readback/async call, ZIP checksum and license contents |
| Source installation of both CLI executables | Passed: both installed and report version 0.10.0 |
| Native desktop release build/package | Passed on macOS arm64; all four executables and license materials included |

## External gates

Hosted Linux/macOS CI for these changes cannot be claimed as passed before the
commits are pushed. Both release workflows require the reusable checks for the
exact resolved release commit. No tag or release was published.

The repository remains private. Branch-protection inspection returned a
plan/visibility restriction; release-environment and private-reporting checks
returned 404. Maintainers must verify the settings listed in
[release readiness](release-readiness.md) before declaring the public launch
ready. A clean scanner result does not prove absence of undiscovered secrets.

The main README is `docs/README.md`; no root README is retained.

No feature commits or push have been performed; the proposed feature sequence
requires user confirmation. Unrelated branding, Conductor work, and dependency
upgrades are not part of these changes.
