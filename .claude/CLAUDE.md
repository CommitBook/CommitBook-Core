# CommitBook

Automated git commits and sync for markdown notebooks.

## Build & Test

```bash
cargo build                       # Build all crates
cargo test                        # Run all tests
cargo build -p commitbook-cli     # Build CLI only
cargo test -p commitbook-engine   # Test engine only
```

## Project Structure

Cargo workspace with 5 crates:

```
crates/
  commitbook-engine/   # Shared engine: config, git, AI, state, sync, scheduling, logging
  commitbook-cli/      # CLI binary (commitbook)
  commitbook-tui/      # Terminal dashboard (commitbook-tui)
  commitbook-web/      # Web dashboard (commitbook-web)
  commitbook-mobile/   # Mobile FFI scaffolding (placeholder)
```

All UI crates depend on `commitbook-engine`. No database — all state is file-based in `.CommitBook/`.

## Architecture

- **No database.** State lives in `.CommitBook/` (`config.toml` committed; `local/` gitignored).
- **No global config.** Each repo is self-contained. No `~/.commitbook/`.
- **Explicit init.** `commitbook init` is a separate command. Other commands hard-fail with "CommitBook is not initialized" if `.CommitBook/` is missing.
- **Exactly one remote required.** `init` blocks if the repo has 0 or >1 remotes; the remote's name is persisted in `config.git.remote` (need not be `origin`).
- **Thin git wrapper.** Sync is `git pull --rebase --autostash` → optional commit → `git push`, retrying once on a non-fast-forward push race. Conflicts surface only at the stash-pop step and are resolved by the configured AI CLI (or left as `<<<<<<<` markers in `manual` mode).
- `.CommitBook/` folder always uses capital C and B.

### Key Modules (commitbook-engine)

| Module | Purpose |
|---|---|
| `sync/` | `sync_repository` orchestrator: pull-rebase-autostash → commit → push |
| `state/` | File-based state: `SyncState` (`last_sync_at`, `last_error`), `AuthConfig` |
| `config/` | `LocalConfig` reads/writes `.CommitBook/config.toml` (incl. `[conflict]` and `[sync]`) |
| `ai/` | Commit-message providers + conflict resolvers: Claude, Codex, Copilot, Gemini, Cursor, fallback |
| `git/` | Git operations via git2 + shells to `git` CLI for pull/rebase/merge |
| `cron/` | Scheduler: launchd (macOS), crontab (Linux) |
| `logger/` | File-backed JSON-lines logger under `.CommitBook/local/logs/` |
| `platform/` | `CredentialProvider`, secret store, logger trait |
| `ffi/` | UDL for mobile bindings (placeholder) |

## CLI Commands

```
commitbook init        # Initialize .CommitBook/ (required before any other command)
commitbook sync        # Pull-rebase-autostash + (optional) commit + push
commitbook start       # Install scheduler
commitbook stop        # Stop scheduler
commitbook status      # Show state
commitbook schedule    # Change schedule
commitbook doctor      # Health check
commitbook log         # Activity log
commitbook login       # Store auth token
```

## Conflict resolution

`.CommitBook/config.toml` `[conflict]` section selects the AI CLI invoked when
`git pull --rebase --autostash` leaves conflict markers:

```toml
[conflict]
resolver = "manual"   # manual | claude | codex | copilot | gemini | cursor
```

`manual` (the default) leaves the markers in place; the user resolves with `git status` and re-runs `commitbook sync`. Any other value spawns the corresponding CLI to rewrite each conflicted file; the orchestrator stages the resolved files and finishes the rebase/stash-pop.

## Code Conventions

- **Tests are colocated** in separate `*_tests.rs` files, referenced via `#[cfg(test)] #[path = "..._tests.rs"] mod tests;`. Never write tests inline in source files.
- **No database or ORM.** State is TOML files + directory structure.
- **Rust edition 2021.** `set_var` requires `unsafe` blocks.
- Async traits use `#[async_trait]`.
- Desktop git operations shell out to `git` CLI; mobile builds skip these via `#[cfg(not(any(target_os = "ios", target_os = "android")))]` and reach git through the `CredentialProvider`-based git2 path.

## .CommitBook/ Directory

`config.toml` is the only committed file. Everything else lives under `local/`, which is gitignored as a single entry (`.CommitBook/local/`). Initialization auto-adds this entry to `.gitignore`.

```
.CommitBook/
  config.toml        # Human-editable settings (COMMITTED to git)
  local/             # All local state (GITIGNORED via single entry)
    auth.toml        # Credentials (0o600 permissions)
    state.toml       # Sync state (last_sync_at, last_error)
    logs/            # Activity logs
      YYYY-MM-DD.log       # Daily JSON-lines log files
      launchd-stdout.log   # macOS scheduler stdout (when scheduled)
      launchd-stderr.log   # macOS scheduler stderr (when scheduled)
    .lock            # Prevents concurrent sync runs
```

Path helpers in code:
- `LocalConfig::local_dir(repo_path)` — returns `.CommitBook/local/`
- `LocalConfig::logs_dir(repo_path)` — returns `.CommitBook/local/logs/`
- `LocalConfig::lock_path(repo_path)` — returns `.CommitBook/local/.lock`
- `AuthConfig` and `SyncState` take `commitbook_dir` (`.CommitBook/`) and internally join `local/` before their filename
