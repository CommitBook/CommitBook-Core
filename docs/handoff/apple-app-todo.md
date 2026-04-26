# CommitBook-Apple — TODO for FFI integration

Work needed in [`CommitBook/CommitBook-Apple`](https://github.com/CommitBook/CommitBook-Apple) once `commitbook-engine` ships its first xcframework. Track each item in a separate PR.

> **Tag convention reminder:** the upstream `commitbook-engine` repo uses unprefixed tags like `0.1.0`, not `v0.1.0`. `.core-version` and any URLs you build pointing at the engine's GitHub releases must use the bare version.

## 1. Bump `.core-version` to consume the new release

- [ ] Get the published release URL + SHA256 from the engine repo's GitHub release `0.1.0`. The artifact filename is `CommitBookEngine.xcframework.zip`.
- [ ] Update `.core-version` at the repo root:
  - `CORE_VERSION=0.1.0`
  - `XCFRAMEWORK_CHECKSUM=sha256:<value-from-engine-release>`
  - `XCFRAMEWORK_URL=https://github.com/CommitBook/CommitBook-Core/releases/download/0.1.0/CommitBookEngine.xcframework.zip`
- [ ] Update `scripts/fetch-xcframework.sh` so:
  - It validates the framework directory name `CommitBookEngine.xcframework` (was `CommitBookCore.xcframework`).
  - It validates the Swift module `CommitBookEngineFFI` (was `CommitBookCoreFFI`).
- [ ] Run `scripts/fetch-xcframework.sh` locally and confirm `Packages/CommitBookAppleCore/Binaries/CommitBookEngine.xcframework/` lands with three slots: `ios-arm64`, `ios-arm64_x86_64-simulator`, `macos-arm64_x86_64`.

## 2. Update `Package.swift` to point at the new framework

- [ ] In `Packages/CommitBookAppleCore/Package.swift`, change the binary target name from `CommitBookCoreFFI` to `CommitBookEngineFFI` and the path to `Binaries/CommitBookEngine.xcframework`.
- [ ] Update the conditional that decides whether to add the binary target — should still gate on `Binaries/CommitBookEngine.xcframework` existing locally.
- [ ] Run `swift build` from `Packages/CommitBookAppleCore` to confirm the package compiles with the binary present.

## 3. Rename Swift types: `Workspace*` → `CommitBook*`

The engine FFI is now CommitBook-native. The app's `WorkspaceInput` / `WorkspaceSummary` must follow.

- [ ] `Packages/CommitBookAppleCore/Sources/CommitBookAppleCore/Types.swift`:
  - `WorkspaceInput` → `CommitBookInput`. Trim fields: drop `remoteURL` (engine derives from `owner`+`repo`), `localRoot` (engine derives from `workspacesRoot` + slug). Keep `name`, `mode`, `provider`, `owner`, `repo` (renamed from `repoName`), `branch`.
  - `WorkspaceSummary` → `CommitBookSummary`. Add fields the engine returns: `owner: String`, `repo: String`, `docCount: Int`, `conflictCount: Int`. Note: `docCount`/`conflictCount` come back as `0` from `list_commitbooks` — populate them by calling `list_documents` / `list_conflicts` per book if the UI needs accurate counts.
  - Add new types: `DiscoveredCommitBook { owner, repo, defaultBranch, isPrivate, hasDotCommitbook, alreadyLocal }`, `RepoInfo { owner, name, defaultBranch, isPrivate }`, `SyncMode { case aiResolve, manual }`.
  - `SyncResultSummary`: add `committed: Bool`, `conflictsResolved: Int`, `manualConflicts: Int` (engine returns these now).

- [ ] `Packages/CommitBookAppleCore/Sources/CommitBookAppleCore/CommitBookEngineProtocol.swift`:
  - Rename methods: `createWorkspace` → `createCommitBook(_ input: CommitBookInput, token: String) async throws -> CommitBookSummary`. (Note: `create_commitbook` is now async on the engine side because cloning is a network op.)
  - `listWorkspaces` → `listCommitBooks() throws -> [CommitBookSummary]`.
  - `getWorkspace` → `getCommitBook(_ id: String) throws -> CommitBookSummary`.
  - `deleteWorkspace` → `deleteCommitBook(_ id: String) throws`.
  - `syncWorkspace(_ id: String) async throws -> SyncResultSummary` → `syncCommitBook(_ id: String, mode: SyncMode, token: String) async throws -> SyncResultSummary` — note the new `mode` and `token` parameters.
  - Add `validatePAT(_ token: String) async throws -> [RepoInfo]` (was already in the protocol per the current file).
  - Add `discoverCommitBooks(_ token: String) async throws -> [DiscoveredCommitBook]` — new, lets the user pick which repo to register.
  - Document `id` is `<owner>/<repo>`.

## 4. Implement `RealEngine.swift` against the new FFI

- [ ] Replace the body of every method in `RealEngine.swift` with a call into the UniFFI-generated `CommitBookEngineClient`. Most methods are thin pass-throughs.
- [ ] `RealEngine.makeDefault()` should construct `CommitBookEngineClient(dbPath:, workspacesRoot:)` with paths under the app's `Application Support` directory:
  - `dbPath` → `<AppSupport>/CommitBook/db/`
  - `workspacesRoot` → `<AppSupport>/CommitBook/repos/`
  - Create both directories if missing.
- [ ] Replace the import: `import CommitBookCoreFFI` → `import CommitBookEngineFFI`.
- [ ] Update the `#if canImport(CommitBookCoreFFI)` guard to `#if canImport(CommitBookEngineFFI)`.

## 5. Update `FFIMapper.swift` and `FFIErrorAdapter.swift`

- [ ] `FFIMapper.swift`: rename `FFIWorkspaceInput`/`FFIWorkspaceSummary` typealiases. Update field names in mappers to match the new contract (`owner`, `repo`, `docCount`, `conflictCount` etc.).
- [ ] Add mappers for `DiscoveredCommitBook`, `RepoInfo`, `SyncMode`.
- [ ] `FFIErrorAdapter.swift`: error variants are unchanged (`databaseError`, `transportError`, `mergeError`, `authError`, `notFound`, `invalidInput`) — but each variant now has an associated `message: String`. Update extraction to use `message`.

## 6. Update `MockEngine.swift` to match the new protocol

- [ ] Add stub returns for `discoverCommitBooks`, the new fields on `CommitBookSummary`, `SyncMode` parameter on `syncCommitBook`. The Mock should keep enough behavior for UI development without the FFI present.

## 7. Rename screens / view models

- [ ] `WorkspaceListView.swift` → `CommitBookListView.swift`. Update internal references.
- [ ] `WorkspaceAddView.swift` → `CommitBookAddView.swift`.
- [ ] `RepoPickerView.swift` — wire to `discoverCommitBooks` so the picker shows existing CommitBooks across the user's GitHub repos at the top, with non-CommitBook repos below.
- [ ] `WorkspaceService.swift` → `CommitBookService.swift`. Update references in `AppState`.
- [ ] Search for `Workspace` across the entire app and rename mechanically. Probably ~20 files touched.

## 8. Update conflict UI for dual-mode sync

The engine now exposes a `SyncMode { aiResolve, manual }` flag per sync. Decide app-side default per platform/screen:

- [ ] Pick the default mode per-sync. Suggested:
  - Background sync (BGAppRefreshTaskRequest): `aiResolve` (no UI, transparent).
  - User-initiated sync from the toolbar: `aiResolve` with a "review conflicts" hint if `manualConflicts > 0` in the result.
  - Conflict review screen → "Sync (manual)" button calls with `manual`.
- [ ] `ConflictListView.swift` already iterates `[ConflictSummary]` — should still work since the engine type matches.
- [ ] `ConflictDetailView.swift`: ensure the resolution buttons map to the four resolution types: `take_local`, `take_remote`, `keep_both`, `manual_edit`.

## 9. PAT entry flow

- [ ] `PATEntryView.swift`: after the user submits, call `validatePAT(token)` and store the token in Keychain via `KeychainHelper`. On success, transition to `RepoPickerView`.
- [ ] `RepoPickerView.swift`: call `discoverCommitBooks(token)`. Group results: existing CommitBooks (`hasDotCommitBook == true`) at the top with an "Add" or "Already added" affordance per `alreadyLocal`; non-CommitBook repos below with a "Create CommitBook" action that calls `createCommitBook(input, token)`.

## 10. CI updates

- [ ] `.github/workflows/ci.yml`: the `real-core-integration` job's gated trigger should fire on a real `.core-version` (not the placeholder). Confirm it works against the new artifact.
- [ ] `EngineContractTests` in `Packages/CommitBookAppleCore/Tests/` — add tests for the new methods (`discoverCommitBooks`, `validatePAT`, `syncCommitBook` with both modes).

## 11. End-to-end smoke test

- [ ] Boot iOS simulator with the app pointing at the new xcframework. Sign in with a real PAT against a throwaway GitHub account.
- [ ] Confirm `discoverCommitBooks` lists your repos, with any pre-existing `.CommitBook/` ones flagged.
- [ ] Create a fresh CommitBook from a new repo. Confirm `.CommitBook/config.toml` shows up on GitHub.
- [ ] Edit a markdown doc in the editor, save. Confirm a local commit happens.
- [ ] Tap Sync. Confirm push lands on GitHub. Confirm signed commits show as ✅ Verified if the simulator's user has signing configured (rare on iOS — usually unsigned, which is fine).
- [ ] Repeat on a real device (different account/CommitBook to avoid clashes).

## Reminders for the engine repo

These are owned by the engine repo, but the Apple repo waits on them:

- [ ] First real release tagged `0.1.0` (no `v` prefix) with `CommitBookEngine.xcframework.zip` attached and SHA256 in release notes.
- [ ] Future releases bump to `0.1.1` / `0.2.0` etc. — Apple repo bumps `.core-version` per release.
