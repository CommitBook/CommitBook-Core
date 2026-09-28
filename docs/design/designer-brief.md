# CommitBook: Product Designer Brief

## What CommitBook is

CommitBook turns any git repository of markdown notes into a self-syncing CommitBook. A user writes notes in their editor of choice; CommitBook runs in the background and, on a schedule the user picks, commits all non-ignored Git changes first, then fetches the latest from the remote, runs an in-process libgit2 3-way merge, and pushes when auto-push is enabled. Desktop AI features invoke an installed CLI such as Claude, Copilot, Codex, Gemini, or Cursor. Mobile conflict resolution calls back into the same Swift/Kotlin app, which talks to its AI service from the phone; it never depends on a desktop app.

Each repository is self-contained. There is no global config, no database, and no central server. State lives in a `.CommitBook/` directory inside the repo (`config.toml` and the nested `.gitignore` are committed; `/local/` is ignored). Initialization is explicit: `commitbook init` must be run before any other command works.

The product spans four user-facing surfaces, all built on the same Rust engine (`commitbook-engine`):

1. **CLI**: `commitbook` binary, primary daily-driver for power users
2. **TUI**: `commitbook-tui`, terminal dashboard built on Ratatui
3. **Web**: `commitbook-web`, local browser dashboard built on Axum + Askama + HTMX
4. **Native apps**: iOS / macOS (CommitBook-Apple) and Android (CommitBook-Android), each a separate repo that consumes the engine through a UniFFI binding (xcframework / AAR)

The design system must cover all four surfaces, but the iOS/macOS and Android apps are where the design investment is highest: those are the consumer-facing products and the only surfaces that non-technical users will ever see.

## Who uses it

- **Power users / developers**: live in the CLI and TUI. Already comfortable with terminals, git, and dotfiles. Care about density, keyboard control, and seeing logs.
- **Note-takers using the native apps**: may not know what git is. They see a list of CommitBooks, open one, type, and trust that their notes are being saved and synced. Conflicts and sync state must be legible without git vocabulary.
- **Self-hosters / tinkerers**: open the local web dashboard occasionally to check the scheduler, read logs, and tweak settings.

## Core domain concepts the UI must visualize

These concepts recur across every surface. The design system needs a consistent visual treatment for each:

- **CommitBook**: a local clone of a Git repository on a provider (GitHub, GitLab, Codeberg, generic git). The eight-character `commitbook_local_id` selects this clone; its remote URL and provider are descriptive Git metadata, not its local identity. It has a name, a branch, a sync mode, an auto-sync flag, a doc count, and a conflict count.
- **Document**: one markdown file inside a CommitBook. Has a path and content.
- **Sync**: one cycle of (commit local changes) → (fetch and libgit2 3-way merge) → (push). Outcome is a `SyncResult` with five numbers the UI may want to surface: `committed` (bool), `pulled` (count), `pushed` (count), `conflictsResolved` (count, AI), `manualConflicts` (count, left for user), plus an `errors` list.
- **Sync mode**: `aiResolve` (let the configured AI rewrite conflicted files) or `manual` (leave conflict markers in place). Picked per-sync; background syncs default to `aiResolve`, user-initiated syncs may prompt.
- **Conflict**: a path with ancestor/local/remote Git index entries. Sides are nullable for add/delete conflicts, and binary or special entries remain manual. Resolutions: take local, take remote, keep both, delete, or manual edit.
- **Schedule**: a 5-field cron expression with friendly presets (every 5/15/30 min, hourly, every 4h, daily at 9am, custom). Display the cron in plain English wherever it appears.
- **Scheduler state**: running, stopped, or broken (a job is installed but its binary is gone). There is no separate enabled flag; stopping the scheduler is how a device pauses.
- **Devices**: every device that syncs a CommitBook has a name and platform, visible to the other devices.
- **AI provider**: Claude, Copilot, Codex, Gemini, Cursor, or a fallback (timestamp-based message). The UI should show which providers are installed/available on the user's machine, and which one is currently selected for commit messages and for conflict resolution (these can differ).
- **Auth**: a personal access token tied to a provider (GitHub, GitLab, etc.), stored locally with restrictive permissions. The UI must support add / replace / clear, and surface "your token is missing or invalid" without leaking the token itself.
- **Activity log**: a JSON-lines stream of events (timestamp, level, message). Levels are debug / info / warn / error. Logs rotate daily; default retention is 30 days. The UI streams new entries in real time on the TUI and web surfaces.
- **Last sync**: timestamp of last successful sync, plus the last error if any. This is the single most important "is everything OK?" indicator across every surface.

## Surfaces in detail

### 1. CLI (`commitbook`)

Nine commands: `init`, `sync`, `start`, `stop`, `status`, `schedule <expr>`, `doctor [--fix]`, `log [-n] [-f]`, `completions <shell>`.

Output is colored text: bold cyan headers, green/yellow/red status glyphs, dimmed secondary text. A `--json` flag produces machine-readable output. There are no spinners, tables, or interactive prompts: everything is a single pass of structured text.

**Designer's role here is small**: pick the palette tokens and status glyphs (✓ / ✗ / ● / ○) so the CLI matches the rest of the system. No layout work.

### 2. TUI (`commitbook-tui`)

A terminal dashboard. Ratatui-based, four panels in a 2x2 grid, rotatable with Tab / Shift+Tab. Keybinds: `q` quit, `r` refresh, `s` toggle scheduler, arrows to scroll.

- **Status panel**: scheduler state, schedule (in English), current branch, pending changes summary, last commit
- **Logs panel**: scrollable, color-coded by level
- **Config panel**: read-only key/value
- **Providers panel**: list of AI providers with availability checkmarks

**Designer's role:** define the terminal palette (it must work in both light and dark terminals), the active-vs-inactive panel border treatment, the log-level color scale, and the empty/loading/error states for each panel.

### 3. Web dashboard (`commitbook-web`)

Local browser dashboard, served by Axum, rendered with Askama templates and updated live with HTMX. Three pages:

- **Dashboard**: status card (refreshes every 5s), providers card (refreshes every 30s), Start/Stop scheduler buttons
- **Config**: form: schedule dropdown with presets + custom cron field, branch input, auto-push toggle, save button
- **Logs**: paginated activity stream

Today's styling is a GitHub-Dark-flavored dark theme (`#0d1117` bg, `#161b22` surfaces, `#58a6ff` accent). The design system should replace this with whatever the new system specifies: but keep in mind HTMX swaps partials in place, so components must look correct without animation chrome.

**Designer's role:** define the full dark + light theme, card / form / button / badge / table primitives, the schedule picker, the logs table, and a treatment for "live-updating" content (subtle pulse? timestamp? nothing?).

### 4. iOS / macOS app (CommitBook-Apple) and Android app (CommitBook-Android)

These are separate repos consuming the engine through UniFFI bindings. The Apple repo exists today with a placeholder UI and a working Mock engine; the Android repo is gated on the engine shipping an AAR. Both apps will need:

- **Onboarding**: paste a personal access token, validate it (the engine returns a list of repos the token can see), pick which repos to register as CommitBooks, and / or discover existing CommitBooks (repos that already have a `.CommitBook/` directory) across the user's account.
- **CommitBook list**: every CommitBook the user has registered, with name, owner/repo, doc count, conflict count, last sync, and a sync state indicator. This is the home screen.
- **Document list**: inside one CommitBook, the markdown files in the repo. Tap to open.
- **Document editor**: markdown editor. Out of scope for the design system's core primitives, but it lives inside the app shell so the chrome (nav, save state, sync indicator) must be defined.
- **Sync UI**: a sync button per CommitBook and a global one. Show progress and the SyncResult outcome (X committed, Y pulled, Z pushed, N conflicts resolved, M to review).
- **Conflict review screen**: list of open conflicts; per conflict, show the path, side-by-side or stacked local vs. remote content, and the four resolution actions (take local / take remote / keep both / manual edit).
- **Settings**: schedule, branch, auto-push, AI provider selection (commit messages and conflict resolver, separately), token management, log retention, about.
- **Activity log**: the same JSON-lines stream the TUI and web show, presented as a scrollable native list.

**Designer's role here is the largest.** This is the consumer surface. The design system needs to specify everything from typography and color through component primitives (cards, lists, forms, modals, sheets, toasts, empty/loading/error states) to flow-level decisions (how onboarding sequences, how conflicts are reviewed, how a sync failure is recovered from).

## States and edge cases the system must handle

- **Empty**: no CommitBooks registered, no documents in a CommitBook, no logs yet, no conflicts.
- **Loading**: sync in flight, fetching repo list during onboarding, validating a token.
- **Error**: token invalid or expired, network unreachable, push rejected (non-fast-forward), AI provider not installed, AI provider crashed mid-resolution, git repository corrupted, `.CommitBook/` missing, lock file held by another process.
- **Partial success**: synced but with N manual conflicts left for the user; committed locally but push failed; pulled and pushed but commit-message generation fell back to the timestamp provider.
- **Background-only**: on iOS the app may sync via `BGAppRefreshTask` with no UI visible; the result must be reflected the next time the user opens the app.
- **Offline**: user opens the app on a plane; can still read and edit, sync is queued.
- **Permission-degraded**: token has read-only scope, can pull but not push.

## Constraints the designer should know up-front

- **Local-first, no central service.** Every surface talks only to the local engine and to the user's git remote. There is no CommitBook account, no leaderboard, no "share" feature, no cloud-side avatars.
- **Repository-scoped.** Settings live per-CommitBook. There is no global "all your CommitBooks behave this way" surface, except in the native apps where the app itself is the global container.
- **Markdown is the only document format.** Don't design for rich images, embedded video, attachments, or non-markdown files.
- **Git semantics leak only when they have to.** Use words like "sync", "save", "conflict", "review changes" rather than "rebase", "fetch", "stash", "HEAD" in the consumer apps. The CLI and TUI can be more git-literal.
- **The CLI / TUI / web surfaces share a system but don't share components.** They render in totally different mediums. Aim for a shared palette and shared iconography vocabulary, not shared components.
- **The native apps must feel native.** The design system should specify when to use platform conventions (iOS sheets, Android Material) and when to override them with a CommitBook-specific component. A user should not feel the app was ported from the other platform.
- **Identity is the repo.** A CommitBook's icon / cover / accent is a great place to add personality, since the underlying object is just `<owner>/<repo>`. Consider whether the designer wants to introduce per-CommitBook color or imagery.

## What I want from the design system

1. **Foundations**: color (light + dark), typography, spacing, radii, elevation, motion, iconography. Tokens should be exportable so the web theme can consume them as CSS variables and the Apple/Android apps can consume them as SwiftUI / Compose theme tokens.
2. **Primitives**: buttons, inputs, selects, toggles, cards, lists, badges, banners, modals, sheets, toasts, tabs, progress, skeletons, empty states.
3. **Domain components**: sync state indicator (the most important component in the system), schedule picker, conflict diff viewer with resolution actions, activity log row, AI provider availability badge, token entry / status, CommitBook card.
4. **Flows**: onboarding, sync-with-conflicts review, schedule change, token rotation, AI provider switch.
5. **Surfaces**: wireframes / hi-fi for: native app (iOS + Android), web dashboard. Token-only handoff for CLI / TUI palettes.

## Reference material

- Repository: `CommitBook-Core` (this repo, the engine).
- Apple app: `CommitBook/CommitBook-Apple`: current UI is a placeholder; rename `Workspace*` → `CommitBook*` is in flight.
- Android app: `CommitBook/CommitBook-Android`: currently on `MockEngine`, gated on engine AAR.
- Tag convention on the engine: bare semver (`0.5.0`), no `v` prefix.
