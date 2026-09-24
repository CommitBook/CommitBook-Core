# Sync Architecture

CommitBook syncs a local clone with a remote git repository. This document records the architecture choice for that sync, the alternatives considered, and the criteria for revisiting the decision.

## Decision

**Merge-based libgit2.** A single sync algorithm shared by desktop and mobile, implemented entirely through the `git2` crate (libgit2 bindings), with no shell-outs to system `git`. Conflicts are surfaced through a per-call `SyncMode` flag that lets the caller pick AI-driven auto-resolution or manual handling.

No configuration option. No automatic fallback to a different algorithm. One path.

## Sync algorithm

```
1. acquire the shared per-repository lock; prepare/migrate .CommitBook/local
2. stage every dirty tracked or non-ignored Git change
3. if index has real changes vs HEAD: create the local snapshot commit
4. fetch <remote> <branch>
5. analyze: merge_analysis(FETCH_HEAD)
6. dispatch on analysis:
     UP_TO_DATE        -> nothing pulled
     FAST_FORWARD      -> preflight collisions, safely checkout changed paths,
                          then publish the branch ref
     NORMAL (3-way)    -> repo.merge(FETCH_HEAD)
                          if index has no conflicts:
                              write_tree, commit with two parents (merge commit)
                          else:
                              if [conflict] auto_merge_appends (default on):
                                  resolve every text conflict whose hunks only add
                                  lines on both sides (blank ancestor section) as
                                  local additions + remote additions; if none remain,
                                  create the merge commit
                              for the remaining conflicts, match SyncMode:
                                  AiResolve -> resolver.resolve(GitConflict) for each text conflict
                                               stage explicit content/deletion results,
                                               write_tree, create one resolved merge commit
                                  Manual    -> preserve merge state and return index-derived
                                               ConflictSummary entries
7. if git.auto_push: push
   on non-fast-forward race: refetch + repeat from step 5 (one retry)
```

The whole flow is sync-callable. Desktop resolvers spawn the configured AI CLI.
On mobile, the Rust engine invokes an asynchronous callback supplied by the
embedding Swift/Kotlin app; that app may call its AI provider or backend over
HTTPS from the phone. Mobile never calls a desktop app.

All entry points that mutate a managed repository use the same RAII lock,
including manual and scheduled sync, FFI sync, document saves, conflict
resolution, and metadata publication.

## Why merge, not rebase

| Concern | Merge | Rebase |
|---|---|---|
| LOC for libgit2 implementation | ~80 | ~200 (state machine via `git2::Rebase`) |
| Linear history | No (merge commits visible) | Yes |
| Conflict mid-flight handling | Single conflict point | Per-step replay; multiple conflict points possible |
| Aborting mid-flight | `cleanup_state()` + reset | Walk the rebase iterator, abort each step |
| Behavior on identical changes both sides | Auto-resolves | Auto-resolves |
| Multi-device commits preserved | Yes (both lineages remain) | Yes (rewritten on top) |

Notebook history granularity is not a product concern. CommitBook autogenerates commits whose content is the value users care about, not the layout of `git log --graph`. That collapses the rebase advantage to "subjectively cleaner log" which we're not buying.

The 2.5× LOC ratio matters more than it sounds: the rebase implementation needs to track an in-progress rebase across calls, handle resume after partial conflicts, and unwind cleanly on errors. The merge implementation is one function with two branches.

## Alternatives considered

### A. Full libgit2 v2 port using `Repository::rebase`

The most direct port of the current shell-out v2 model (`git pull --rebase --autostash`). Preserves linear history exactly. Requires implementing:

- `Repository::stash_save` to autostash dirty working tree before rebasing
- `Repository::rebase` returning an iterator of `RebaseOperation`s; each must be `commit`-ed
- `stash_apply` after rebase finishes; conflicts on apply are the autostash-pop conflicts
- State recovery on abort (the rebase state directory needs explicit cleanup)

**Why not chosen**: implementation cost without proportional user benefit. Linear history is the only difference users would see, and CommitBook's automated commits don't make the linearity especially meaningful.

If a future user segment demands linear history (enterprise audit, e.g.), this option can be added as an opt-in `[sync] strategy = "rebase"` knob without removing the merge default. Not on the M2 critical path.

### C. Custom file-level reconciliation (no git merge primitives)

The old planner approach. For each tracked file: compare local working-tree content to remote content (via `read_blob_at_ref`). If both changed since last sync, run conflict resolver. If only one side changed, take that side. Write result, stage, commit, push.

**Pros**: no rebase or merge state machine. Sidesteps libgit2's merge driver entirely. Plays naturally with section-level merging if we ever bring that back.

**Cons**: re-implements what git's merge already does well: three-way merge with renames, mode changes, binary file detection. We'd lose those for free. And the "compare to last sync" base requires tracking sync state per file (the deleted `git/base.rs` did this), which is exactly the complexity v2 set out to remove.

**Why not chosen**: re-introduces the planner-shaped complexity v2 just deleted.

### D. Plain fetch + ff-only + commit + push

Try to fast-forward only. If non-fast-forward, error out and ask the user to resolve manually (or hand off to AI).

**Pros**: the simplest implementation possible (~60 LOC). Works perfectly when nothing diverged.

**Cons**: any divergence (two devices both edited a CommitBook between syncs) means errors instead of merging. Acceptable for single-user single-device, painful for the actual common case (phone + Mac both writing). Forces users to manually reconcile.

**Why not chosen**: too restrictive for the multi-device use case CommitBook is built for.

### E. Dual-path: shell-out git on desktop, libgit2 on mobile

Keep `pull --rebase --autostash` shell-outs on desktop (proven, fast, well-tested git CLI) and only use libgit2 on iOS/Android.

**Pros**: leverages git's full feature set on desktop where it's available: hooks, `core.autocrlf`, advanced merge drivers, signed commits via `commit.gpgsign`.

**Cons**: doubled implementation, doubled tests, doubled docs. Behavior drift inevitable. We already chose the libgit2-everywhere path in commits `0e0fb67` + `619b6e1` and shipped it; the v2 commit `5ade9d0` accidentally regressed back to shell-outs for the new helpers, which is what this work corrects.

**Why not chosen**: we explicitly rejected dual-path before. The single benefit (git hooks) is fixable with a one-time warning.

### F. Configurable algorithm

Expose `[sync] strategy = "merge" | "rebase" | "reconcile" | "ff_only"` and ship multiple backends.

**Pros**: power users can choose. Lets us A/B test in production.

**Cons**:
- Most users won't read docs, won't have an opinion → paradox of choice
- 4× the implementation, test surface, documentation
- Different devices for the same user could pick different strategies → inconsistent history
- We'd be guessing what users want before we have any data

**Why not chosen**: shipping one good default beats shipping four mediocre options. If real-world usage produces complaints, we can add a knob then with the data.

## Why no automatic fallback

A tempting pattern: "try rebase, fall back to merge if rebase fails." But:

- "Rebase failed" needs precise definition. Most failures (conflicts) are normal behavior the AI resolver handles, not an architecture-level fallback condition.
- Once a fallback fires, history is non-deterministic. User's Tuesday morning sync rebases; afternoon hits a transient lock and merges. The history mixes two semantics.
- Debugging doubles: "why did it pick merge?" requires reproducing the failure path.
- We'd ship two implementations to support one user-visible behavior: same cost as configurability, less benefit.

The valid form of fallback is *within* an architecture. When `Repository::merge` reports conflicts, we hand them to the resolver; if the resolver returns "manual", we surface them to the caller. That's normal control flow within the merge architecture, not a fallback to a different one.

## Conflict surface: `SyncMode`

Every `sync_commitbook` call takes a mode:

```rust
pub enum SyncMode {
    /// Auto-resolve conflicts via the configured AI resolver.
    /// Surface as `manual_conflicts` only if the resolver fails.
    AiResolve,
    /// Don't auto-resolve. Leave conflict markers in working tree
    /// and return ConflictSummary entries for the caller to handle.
    Manual,
}
```

Apps decide per-sync. Typical patterns:
- Mobile apps default to `AiResolve` for transparent background sync. Opt into `Manual` from a "review conflicts" UI.
- Desktop CLI defaults to whatever `[conflict] resolver` says: `manual` → `Manual`; otherwise → `AiResolve`.

`AiResolve` can complete a conflict-free mobile sync without a callback. If a
text conflict occurs and no callback is registered, the engine preserves the
merge and returns an actionable configuration error with the manual conflict
summaries. Binary, symlink, and gitlink conflicts always remain manual.

Apps' existing `ConflictListView` / `ConflictDetailView` activate when `Manual` mode returns conflict entries.

## Append-only conflicts

Notebook conflicts are most often two devices appending to the same note while
apart. Before any resolver runs, `GitRepo::try_resolve_append_only` probes a
diff3 merge of the three index stages. When every conflicting hunk has an empty
or whitespace-only ancestor section (nothing existing was edited or removed),
it stages a libgit2 union merge: local additions first, then remote additions.
Any other hunk, a delete/modify pair, or a binary or special file leaves the
conflict untouched for the configured resolver or the user. The same pass also
runs when a later cycle recovers a merge left in progress. Set
`[conflict] auto_merge_appends = false` to treat these as ordinary conflicts.

## Optional review of AI resolutions

`[conflict] review_ai_resolutions = true` changes what happens after the resolver returns. Instead of applying the resolution, the engine validates it (no leftover markers, not a binary or special file) and stores it in `.CommitBook/local/conflict-proposals.toml` together with the merge identity (HEAD and merge-parent OIDs) and each side's OID and mode. The index and working tree stay conflicted, the cycle ends as "awaiting review", and nothing is pushed.

Later cycles reuse pending proposals and never regenerate rejected ones; an explicit regenerate action retries the resolver. Proposals for a different merge are dropped. The web editor shows the proposal next to the original sides and offers accept, edit and accept, reject, or manual resolution. Each displayed conflict carries a revision token over the merge identity, conflict sides, and current working-tree content; mutations revalidate it under the repository lock and reject stale requests. Native sync honors review mode by reporting pending reviews as manual conflicts through the existing result fields.

Resolving the final conflict creates the merge commit locally. Publication is a separate step (`Sync now` or the scheduler) that follows `git.auto_push`.

## When to revisit

Reasons to add a second algorithm (rebase) as opt-in:
- Multiple users explicitly request linear history via support channels
- Enterprise customer requirement (audit log expects linear progression)
- A bug in libgit2's merge breaks something rebase wouldn't

Reasons to add automatic fallback:
- Specific reproducible failure mode in libgit2 merge that doesn't affect rebase

Reasons to switch the default away from merge:
- Empirically merge commits are confusing to a meaningful percentage of users
- Performance regression in libgit2 merge that doesn't affect alternatives

Until any of those hold, the answer is "merge, no config, no fallback".

## History examples

### Normal flow

```
*   3a4b5c6 (HEAD -> main) Merge branch 'main' of github.com:user/notes
|\
| * d7e8f90 (origin/main) Edit Daily-Notes/2026-04-25.md from iPad
* | a1b2c3d Edit Daily-Notes/2026-04-25.md via CommitBook
|/
* 9876abc Initial CommitBook setup
```

Two devices touched the same CommitBook between syncs. No conflict (different sections of the same file or different files). Merge commit at the top.

### Conflicted flow with AiResolve mode

```
*   7e8f9a0 (HEAD -> main, origin/main) Merge branch 'main' of github.com:user/notes
|\
| * d7e8f90 Edit Daily-Notes/2026-04-25.md from iPad
* | a1b2c3d Edit Daily-Notes/2026-04-25.md via CommitBook
|/
* 9876abc Initial CommitBook setup
```

The resolver works from the ancestor/local/remote entries in the Git index.
The resolved tree is written directly into a single two-parent merge commit;
CommitBook never commits conflict markers as an intermediate history entry.

### Conflicted flow with Manual mode

`sync_commitbook` returns immediately with `manual_conflicts: 1` and an
index-derived `ConflictSummary` for the path. Nullable sides accurately
represent add/delete conflicts and binary/special entries are identified. The
caller takes over until `resolve_conflict` stages the selected index side,
explicit deletion, or validated text and finishes the one merge commit.

## Compatibility with existing repos

Notebooks already synced under v2-shellout-rebase have linear histories. After this change, future merges will produce merge commits. There is no migration step: git accepts both shapes coexisting in the same repo. Users who care about linearity can still `git rebase -i` manually if they want.

## Implementation pointers

- `commitbook-engine/src/git/operations.rs`: owns libgit2 fetch, safe fast-forward, merge, and push primitives.
- `commitbook-engine/src/git/conflicts.rs`: reads structured ancestor/local/remote index entries and applies explicit resolutions.
- `commitbook-engine/src/sync/scheduler.rs`: the `SyncMode` parameter threads from FFI calls down to the merge handler; records the failing stage and timestamps in `state.toml`.
- `commitbook-engine/src/review.rs`: proposal storage, revision tokens, and accept/reject/regenerate actions.
- `commitbook-engine/src/inspection.rs`: read-only repository status and commit preview shared by CLI, web, and TUI.
- Both desktop and mobile share the same code path; no `#[cfg(target_os = ...)]` guards on this module.
