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
  commitbook-client/   # Mobile FFI SDK (UniFFI)
```

All UI crates depend on `commitbook-engine`. No database: all state is file-based in `.CommitBook/`.

## Architecture

- **No database.** State lives in `.CommitBook/` (`config.toml` and `devices/` committed; `local/` gitignored).
- **No global config.** Each repo is self-contained. No `~/.commitbook/`.
- **Explicit init.** `commitbook init` is a separate command. Other commands hard-fail with "CommitBook is not initialized" if `.CommitBook/` is missing.
- **Exactly one remote required.** `init` blocks if the repo has 0 or >1 remotes; the remote's name is persisted in `config.git.remote` (need not be `origin`).
- **libgit2 merge-based sync.** Sync commits every dirty, non-ignored change first, then fetches, runs an in-process libgit2 3-way merge (fast-forward, true merge, or surfaced conflicts), then pushes, retrying once on a non-fast-forward push race. Same single code path on desktop and mobile. Conflicts surface at the merge step and are handled per `[conflicts] mode` (see below). Sync always pushes; there is no local-only mode. Sync refuses to run while a user-started git operation is in progress (any repository state other than clean or a merge of the configured remote branch, or index conflicts outside a merge); see `ensure_no_user_operation`.
- **One sync command.** The scheduler (launchd/cron) runs `commitbook sync`, the same command users run. Failures and a contended lock exit non-zero.
- `.CommitBook/` folder always uses capital C and B.

### Key Modules (commitbook-engine)

| Module | Purpose |
|---|---|
| `sync/` | `sync_repository` orchestrator: commit dirty changes → fetch → libgit2 3-way merge → push |
| `state/` | File-based state: `SyncState` (`last_sync_at`, `last_error`), `AuthConfig` |
| `config/` | `LocalConfig` reads/writes `.CommitBook/config.toml` (comment-preserving via `toml_edit`); `values.rs` holds the enumerated values |
| `devices/` | Committed per-device files `.CommitBook/devices/<id>.toml` (`name`, `platform`, `auth`) |
| `ai/` | Commit-message providers + conflict resolvers: Claude, Codex, Copilot, Gemini, Cursor, fallback |
| `git/` | Git operations via git2 (libgit2): fetch, merge, commit, push |
| `cron/` | Scheduler: launchd (macOS), crontab (Linux) |
| `logger/` | File-backed JSON-lines logger under `.CommitBook/local/logs/` |
| `platform/` | `CredentialProvider`, secret store, logger trait |

## CLI Commands

```
commitbook init        # Initialize .CommitBook/ (required first; asks before commit + push, --yes skips)
commitbook sync        # Commit dirty changes + libgit2 merge + push
commitbook start       # Install scheduler
commitbook stop        # Stop scheduler
commitbook status      # Show local commit, remote, scheduler, devices, and last sync state
commitbook devices     # List devices; `rename <name>` renames this one, `remove <id>` another
commitbook preview     # List what the next sync would commit (read-only, --json)
commitbook schedule    # Change schedule
commitbook doctor      # Health check
commitbook log         # Activity log
commitbook completions # Generate shell completion scripts
```

## Config file

`.CommitBook/config.toml` is committed and shared by every device. `init` writes it from a commented template; `LocalConfig::save` edits values in place so user comments survive. Unknown keys and invalid values are rejected. There is no legacy migration: a file without `[config] schema = 1` fails with "delete it and run `commitbook init`".

```toml
[config]
schema = 1              # file format; bumped only for incompatible layout changes

[commitbook]
name = "notes"

[git]
branch = "main"
remote = "origin"       # provider/owner/repo are parsed from this remote's URL (git::remote::remote_identity)

[sync]
schedule = "1h"         # stored as picked (1h, 15m, daily) or cron; cron::to_cron converts at install

[commit]
mode = "timestamp"      # timestamp | ai
agent = "any"           # any | claude | codex | copilot | gemini | cursor

[conflicts]
mode = "both"           # both | manual | ai | review
agent = "claude"        # claude | codex | copilot | gemini | cursor

[logs]
keep = "30d"            # <N>d | forever
```

## Conflict resolution

`[conflicts] mode` decides what happens when the libgit2 3-way merge leaves conflicts:

- `both` (default): conflicted Markdown/text notes (`.md`, `.markdown`, `.txt`) are resolved with a union merge that keeps both versions without markers, local first (`GitRepo::try_resolve_both`). The files are listed in `SyncOutcome.kept_both` and `state.toml` (`kept_both_paths`, `kept_both_at`). Other files, binary files, and delete/modify conflicts are handled like `manual`.
- `manual`: markers stay in place; the user resolves with `git status` and re-runs `commitbook sync`, or uses the web dashboard `/conflicts` editor (use local, use remote, keep both, delete, save edited text).
- `ai`: `[conflicts] agent` rewrites each conflicted file; the orchestrator stages the resolved files and finishes the merge commit.
- `review`: sync stores each AI proposal in `.CommitBook/local/conflict-proposals.toml` and stops without applying it or pushing. Pending and rejected proposals are not regenerated by later cycles; the web editor accepts, edits, rejects, or regenerates them. Switching mode does not approve stored proposals. Resolving the last conflict creates a local merge commit; the next sync publishes it.

## Commit messages

`[commit] mode = "timestamp"` (default) uses only the deterministic `FallbackProvider` (a `Writing <timestamp> (...)` message) and never spawns an AI CLI. `mode = "ai"` asks `[commit] agent` (or, for `any`, Copilot, Claude, Codex, Gemini, Cursor in order), then falls back to the timestamp. Key selection lives in `commitbook_engine::ai::commit_provider_keys`.

## Status, preview, and settings

- Scheduler state comes from `cron::health`: `stopped`, `running`, or `broken` when the job's binary no longer exists. `SchedulerHealth::warning` also flags a job with no sync attempt for three schedule intervals. `start` and `doctor --fix` warn when installing a `target/debug` or `target/release` binary.
- `commitbook status`, the web `/api/status` endpoint, and the TUI all read the shared engine status service. It reports the real HEAD commit, dirty files, cached ahead/behind counts, merge/conflict state, pending AI reviews, and the `state.toml` timestamps. It never fetches, invokes AI, takes the lock, or rewrites config.
- Status also lists the devices from `.CommitBook/devices/` and the notes where `both` mode last kept two versions.
- `last_sync_at` marks a successful cycle, not a push. `last_attempt_at`, `last_fetch_at`, `last_push_at`, and `last_error_stage` distinguish attempts, remote checks, publication, and the failing stage.
- `commitbook preview`, web `/changes`, and the TUI `p` screen show what normal staging would commit: every non-ignored Git file of any type, not only Markdown. Preview writes nothing.
- Settings changes from CLI, web, and TUI go through `commitbook-engine/src/settings/` under the repository lock; config writes are atomic and an active scheduler is reinstalled (or rolled back) when the schedule changes. A branch change is accepted only when that branch is checked out; `[git] remote` is not editable after `init`.

## Code Conventions

- **Tests are colocated** in separate `*_tests.rs` files, referenced via `#[cfg(test)] #[path = "..._tests.rs"] mod tests;`. Never write tests inline in source files.
- **No database or ORM.** State is TOML files + directory structure.
- **Rust edition 2021.** `set_var` requires `unsafe` blocks.
- Async traits use `#[async_trait]`.
- Every subprocess spawned during sync (AI CLIs, `gh` probes, `gpg`/`ssh-keygen` signing) goes through `process::run_bounded`, which enforces one deadline covering exit and output draining and kills the process group on timeout. Never call `.output()` or `wait_with_output()` there.
- Desktop git operations shell out to `git` CLI; mobile builds skip these via `#[cfg(not(any(target_os = "ios", target_os = "android")))]` and reach git through the `CredentialProvider`-based git2 path.

## .CommitBook/ Directory

`config.toml`, `.gitignore`, and `devices/` are committed. Everything else lives under `local/`, which is gitignored as a single entry: initialization writes `/local/` into the committed `.CommitBook/.gitignore` (the repository-root `.gitignore` is never changed). Every sync restores that entry if a merge removed it, and `GitRepo::stage_all`/`stage_paths` never stage `.CommitBook/local/` regardless of ignore rules.

Each device writes only its own `devices/<id>.toml`, and only when it registers (`commitbook init`, or the first sync on a clone that never ran init) or is renamed, so device files never conflict or cause commits on their own.

```
.CommitBook/
  config.toml        # Human-editable settings (COMMITTED to git)
  .gitignore         # Contains /local/ (COMMITTED to git)
  devices/           # One file per device: name, platform, auth (COMMITTED to git)
  local/             # All local state (GITIGNORED via single entry)
    device-id        # This device's id (names its devices/<id>.toml)
    auth.toml        # Credentials (0o600 permissions)
    state.toml       # Sync state (last_sync_at, last_attempt_at, last_fetch_at, last_push_at, last_error, last_error_stage, kept_both_paths, kept_both_at)
    preferences.toml # Mobile per-device preferences (auto_sync)
    conflict-proposals.toml  # Stored AI conflict proposals awaiting review (review mode)
    logs/            # Activity logs
      YYYY-MM-DD.log       # Daily JSON-lines log files
      launchd-stdout.log   # macOS scheduler stdout (when scheduled)
      launchd-stderr.log   # macOS scheduler stderr (when scheduled)
    .lock            # Prevents concurrent sync runs
```

Path helpers in code:
- `LocalConfig::local_dir(repo_path)`: returns `.CommitBook/local/`
- `LocalConfig::logs_dir(repo_path)`: returns `.CommitBook/local/logs/`
- `LocalConfig::lock_path(repo_path)`: returns `.CommitBook/local/.lock`
- `AuthConfig` and `SyncState` take `commitbook_dir` (`.CommitBook/`) and internally join `local/` before their filename
