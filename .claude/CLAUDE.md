# CommitBook

Automated git commits and sync for markdown notebooks.

## Build & Test

```bash
cargo build                    # Build all crates
cargo test                     # Run all tests (270+ tests)
cargo build -p commitbook-cli  # Build CLI only
cargo test -p commitbook-core  # Test core only
```

## Project Structure

Cargo workspace with 4 crates:

```
crates/
  commitbook-core/   # Core library: config, git, AI, state, sync, merge, transport
  commitbook-cli/    # CLI binary (commitbook)
  commitbook-tui/    # Terminal dashboard (commitbook-tui)
  commitbook-web/    # Web dashboard (commitbook-web)
```

All UI crates depend on `commitbook-core`. No database — all state is file-based in `.CommitBook/`.

## Architecture

- **No database.** State lives in `.CommitBook/` (config.toml, state.toml, auth.toml, base/ versions).
- **No global config.** Each repo is self-contained. No `~/.commitbook/`.
- **Auto-init.** CLI auto-creates `.CommitBook/` on first use. No `init` command.
- **Sync = commit + try-push.** Every sync commits locally (works offline), then pulls/merges/pushes if remote is reachable.
- `.CommitBook/` folder always uses capital C and B.

### Key Modules (commitbook-core)

| Module | Purpose |
|---|---|
| `state/` | File-based state: SyncState, AuthConfig, base version management |
| `sync/` | Planner (creates sync plan), pipeline (executes pull/merge/push), scheduler |
| `merge/` | Section-aware three-way merge engine for markdown |
| `transport/` | RemoteTransport trait: local_repo, ssh_git, github_pat, github_app |
| `config/` | LocalConfig reads/writes `.CommitBook/config.toml` |
| `ai/` | Commit message generation: Copilot, Claude, Codex, fallback |
| `git/` | Git operations via git2 + CLI |
| `cron/` | Scheduler: launchd (macOS), crontab (Linux) |
| `markdown/` | Parser, reassembler, frontmatter, section tree |

## CLI Commands

```
commitbook sync        # Commit locally + pull/merge/push
commitbook start       # Install scheduler
commitbook stop        # Stop scheduler
commitbook status      # Show state
commitbook schedule    # Change schedule
commitbook doctor      # Health check
commitbook conflicts   # Show merge conflicts
commitbook log         # Activity log
commitbook login       # Store auth token
```

## Code Conventions

- **Tests are colocated** in separate `*_tests.rs` files, referenced via `#[cfg(test)] #[path = "..._tests.rs"] mod tests;`. Never write tests inline in source files.
- **No database or ORM.** State is TOML files + directory structure.
- **Rust edition 2021.** `set_var` requires `unsafe` blocks.
- Transport implementations use `#[async_trait]`.
- Test transports use `transport/mock.rs` (`MockTransport`), available only under `#[cfg(test)]`.

## .CommitBook/ Directory

```
.CommitBook/
  config.toml    # Human-editable settings (COMMITTED to git)
  auth.toml      # Credentials (GITIGNORED)
  state.toml     # Sync checkpoint (GITIGNORED)
  base/          # Base versions for 3-way merge (GITIGNORED)
  logs/          # Activity logs (GITIGNORED)
  .lock          # Prevents concurrent runs (GITIGNORED)
```
