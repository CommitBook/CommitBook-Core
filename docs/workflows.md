# GitHub Actions stages

CommitBook uses the staged workflow names from git-same, with CommitBook's
existing automatic triggers and release gates. All release versions are strict
bare `MAJOR.MINOR.PATCH` tags, for example `1.2.3`, never `v1.2.3`. Leading zeros,
prereleases, and build suffixes are rejected. Tags must match the workspace
version and resolve to the exact commit being built or published.

| Workflow | Triggers | Purpose |
|---|---|---|
| S1 - Test CI | Push/PR to `main`, manual, reusable | MSRV, formatting, Clippy, Linux/macOS tests, release-tool tests, iOS checks, four desktop release-build checks, coverage, non-blocking Linux beta canary |
| S2 - Release GitHub | Bare version tag push, manual | Run S1 and audit against one resolved commit, then package and publish desktop archives |
| S3 - Publish Homebrew | Manual, existing release tag | Verify release archives, validate/install/test the formula, then update `CommitBook/homebrew-tap` |
| Security Audit | Push/PR to `main`, daily, manual, reusable | RustSec audit and full-history secret scan |
| S5 - Build XCFramework | Manual | Exact-commit S1/audit gates, framework build, Swift smoke test, optional release upload |

S1's beta job is advisory; stable checks and coverage generation remain gates.
Download HTML and LCOV reports from the workflow's `coverage-<run>-<attempt>`
artifact. No Codecov account or token is used. S1 and S2 call the same
`scripts/release/build-desktop.sh` with locked dependencies and vendored TLS.
Desktop archive filenames, contents, and installer behavior are unchanged.

## Running stages

Run CI on a branch:

```sh
gh workflow run S1-Test-CI.yml --ref my-branch
```

S2 runs automatically when an existing, approved release tag is pushed. To rerun
it manually against that exact tag (the tag must already exist):

```sh
gh workflow run S2-Release-GitHub.yml --ref 1.2.3 -f ref=1.2.3 -f tag_name=1.2.3
```

After S2 has published the release and all assets, explicitly run S3:

```sh
gh workflow run S3-Publish-Homebrew.yml --ref 1.2.3 -f tag=1.2.3
```

The selected workflow ref must contain the new workflows. Running from the
release tag also accommodates the `release` environment's tag-only deployment
policy. S3 does not create tags, rebuild binaries, or publish to crates.io.

S4 (crates.io) is deferred. S5 is a separate, optional Apple SDK release stage:
S2 does not invoke it, and Homebrew does not require it. To build and attach the
XCFramework ZIP and checksum to an existing release of the same version:

```sh
gh workflow run S5-Build-XCFramework.yml --ref 1.2.3 -f ref=1.2.3 -f release_tag=1.2.3
```

Omit `release_tag` for a build-only workflow artifact. S5 checks that its tag
matches both the workspace version and the built commit before publication.
The Apple app repo owns downstream consumption tests against its pinned SDK;
it does not build or publish this Core artifact.


## Homebrew setup and publication

- Destination: `CommitBook/homebrew-tap`, default branch,
  `Formula/commitbook.rb`. No cask is generated.
- Keep `HOMEBREW_TAP_DEPLOY_KEY` as a secret of the Core `release` environment,
  so only jobs that pass that environment's policy can read it. Its public half
  is a write deploy key on `CommitBook/homebrew-tap`; organization deploy keys
  must remain enabled. The normal `GITHUB_TOKEN` reads this source repository's
  release but cannot write to the separate tap. A deploy key is used instead of
  a personal access token because it reaches only the tap, belongs to no
  person, and does not expire.
- Keep the `release` environment restricted to `main` and version-shaped tags.
  S3's publish job uses that environment. If a branch rule or ruleset protects
  the tap's default branch, add the deploy key as a bypass actor so S3 can push.
- Release assets must be publicly downloadable for Homebrew users. S3 downloads
  and checks all four archives against `SHA256SUMS`, verifies their exact file
  layout and license files, then installs through the formula's public URL on a
  native macOS runner. Private or incomplete releases cannot pass this test.
- Formula validation includes Ruby syntax, Homebrew style/audit, installation,
  and `brew test`: all four binaries must report the release version and both
  license files must be installed. Only the native macOS binary is executed;
  other architectures receive checksum and archive-layout checks.
- Only the verified formula artifact reaches the publish job. Publication is
  serialized; rerunning the same version skips committing if nothing changed.
  A failed push fails the job rather than overwriting concurrent tap changes.
  S3 refuses to publish a version older than the one already in the tap.

Once S3 has published the first formula, users can install with:

```sh
brew install CommitBook/tap/commitbook
```

This installs `commitbook`, `cobo`, `commitbook-tui`, `commitbook-web`, and the
license notices. No service or scheduled sync is started by installation.

## Local validation

```sh
actionlint
python3 -m unittest discover -s scripts/release -p 'test_*.py'
bash -n scripts/release/build-desktop.sh
```

Tests cover strict tags, exact-commit validation, all four formula URLs and
checksums, missing/corrupt/unsafe assets, archive installation, and failure
without formula output. A generated formula can also be checked with `ruby -c`
and `brew style`. Hosted build, coverage, and real-release Homebrew installation
checks run only after the workflows are pushed; local fixture tests do not
constitute a published-release smoke test.

If branch protection refers to renamed workflow/check contexts, update those
required checks after the first S1 run. Existing job names are retained where
possible; no repository settings are changed by these source files.
