# Contributing to CommitBook

CommitBook is pre-launch. Prefer one current behavior over compatibility aliases
or migrations, and discuss public SDK changes before expanding their scope.

The main user guide is [docs/README.md](docs/README.md).

## Development

Install Rust 1.91 or newer and Git. Desktop development is supported on macOS
and Linux; Python 3.11+ runs release-tool tests. Native SDK builds additionally
require the platform toolchain. No hosted database or service is required.

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo +1.91.0 check --workspace --all-targets --locked
python3 -m unittest discover -s scripts/release -p 'test_*.py'
cargo audit --deny warnings
```

Use disposable repositories for sync tests: initialization can commit and push,
and sync includes every non-ignored Git file, not only Markdown. Unit/integration
tests use temporary local repositories and fake schedulers rather than installing
real launchd/cron jobs. Tests belong in adjacent `*_tests.rs` files or integration
test directories. Keep assertions about observable behavior and data preservation.

## Architecture and conventions

- `commitbook-engine`: shared Git, config, identity, locking, sync, and review logic.
- `commitbook-cli`: shared implementation for `commitbook` and `cobo`.
- `commitbook-tui` / `commitbook-web`: local dashboards using the engine.
- `commitbook-client`: UniFFI SDK for app-hosted operations.

State is TOML plus Git; there is no database or global registry. Shared settings
live in `.CommitBook/config.toml`. Device-local identity, credentials, proposals,
and logs live in ignored `local/`. Never stage this directory or perform repairs
from status/list/preview operations. Serialize mutations with the repository lock.
Keep subprocesses bounded and avoid logging credentials or CommitBook contents.

Native cloning/discovery is GitHub-only; desktop sync supports existing Git
remotes. Apple artifacts are shipped separately from desktop binaries. Android
packaging and crates.io publication are not supported launch channels.

## Pull requests and commits

Describe the user-visible outcome, relevant tests, and any API/format changes.
Split unrelated features into separate commits, each with its tests and docs.
Use imperative titles under 72 characters that explain what and why, such as:
`Add local CommitBook-Ids to distinguish CommitBook clones`.
Avoid unrelated formatting, generated artifacts, and dependency upgrades.

Follow the repository's agent approval rule: propose exact commit messages and
wait for confirmation before committing or pushing. See [release readiness](docs/release-readiness.md)
for packaging checks and GitHub settings that cannot be guaranteed by source code.
For vulnerabilities, follow [SECURITY.md](SECURITY.md), not public issues.
