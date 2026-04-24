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

- **No database.** State lives in `.CommitBook/` (config.toml committed; local/ gitignored with state.toml, auth.toml, base/ versions).
- **No global config.** Each repo is self-contained. No `~/.commitbook/`.
- **Explicit init.** `commitbook init` is a separate command. Other commands hard-fail with "CommitBook is not initialized" if `.CommitBook/` is missing.
- **Exactly one remote required.** `init` blocks if the repo has 0 or >1 remotes; the remote's name is persisted in `config.git.remote` (need not be `origin`).
- **Sync = commit + push.** Every sync commits locally then pulls/merges/pushes against the configured remote.
- `.CommitBook/` folder always uses capital C and B.

### Key Modules (commitbook-core)

| Module | Purpose |
|---|---|
| `state/` | File-based state: SyncState, AuthConfig, base version management |
| `sync/` | Planner (creates sync plan), pipeline (executes pull/merge/push), scheduler |
| `merge/` | Section-aware three-way merge engine for markdown |
| `transport/` | RemoteTransport trait: git_remote, github_pat, github_app |
| `config/` | LocalConfig reads/writes `.CommitBook/config.toml` |
| `ai/` | Commit message generation: Copilot, Claude, Codex, fallback |
| `git/` | Git operations via git2 + CLI |
| `cron/` | Scheduler: launchd (macOS), crontab (Linux) |
| `markdown/` | Parser, reassembler, frontmatter, section tree |

## CLI Commands

```
commitbook init        # Initialize .CommitBook/ (required before any other command)
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

`config.toml` is the only committed file. Everything else lives under `local/`, which is gitignored as a single entry (`.CommitBook/local/`). Initialization auto-adds this entry to `.gitignore`.

```
.CommitBook/
  config.toml        # Human-editable settings (COMMITTED to git)
  local/             # All local state (GITIGNORED via single entry)
    auth.toml        # Credentials (0o600 permissions)
    state.toml       # Sync checkpoint (remote_head, last_sync_at)
    base/            # Base versions of tracked files for 3-way merge
    logs/            # Activity logs
      YYYY-MM-DD.log       # Daily JSON-lines log files
      launchd-stdout.log   # macOS scheduler stdout (when scheduled)
      launchd-stderr.log   # macOS scheduler stderr (when scheduled)
    .lock            # Prevents concurrent sync runs
```

Path helpers in code:
- `state::local_dir(cb_dir)` — returns `.CommitBook/local/`
- `LocalConfig::local_dir(repo_path)` — returns `.CommitBook/local/`
- `LocalConfig::logs_dir(repo_path)` — returns `.CommitBook/local/logs/`
- `LocalConfig::lock_path(repo_path)` — returns `.CommitBook/local/.lock`
- `AuthConfig`, `SyncState`, `base::*` all take `commitbook_dir` (`.CommitBook/`) and internally join `local/` before their filename
