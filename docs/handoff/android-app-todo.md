# CommitBook-Android, TODO for FFI integration

Work needed in [`CommitBook/CommitBook-Android`](https://github.com/CommitBook/CommitBook-Android) once `commitbook-engine` ships an Android artifact. Track each item in a separate PR.

> **Tag convention reminder:** upstream `commitbook-engine` releases use unprefixed tags like `0.5.0`. URLs and version pins must omit the `v`.

> **iOS goes first.** The first published engine release `0.5.0` ships an iOS-only xcframework. The Android pipeline (AAR or raw `.so` + bindings) lands in a follow-up engine release. Until then, Android stays on `MockEngine`. This TODO covers the *Android-side* work for when that lands.

## 0. Wait for the engine to ship an Android artifact

The engine repo needs to add (in a separate plan):

- [ ] `cargo-ndk` build script producing `aarch64-linux-android`, `armv7-linux-androideabi`, and `x86_64-linux-android` shared libraries.
- [ ] UniFFI Kotlin bindings emitted from the same UDL the iOS pipeline uses.
- [ ] Packaging decision: AAR via GitHub releases vs. publication to Maven Central. AAR is simpler for v1 since it ships a single artifact ready to drop into `app/libs/`.

Track that work upstream. The remaining items here only become actionable once the artifact exists.

## 1. Add the Android artifact dependency

- [ ] Wherever the engine ships the AAR (likely a GitHub release), add a download step or a Gradle dependency. If AAR-via-release: stash it in `app/libs/commitbook-engine-<version>.aar` and reference it from `app/build.gradle.kts`.
- [ ] Add a `.core-version` file at the repo root mirroring the Apple convention:
  - `CORE_VERSION=<published-version>`
  - `AAR_CHECKSUM=sha256:<value>`
  - `AAR_URL=https://github.com/CommitBook/CommitBook-Core/releases/download/<version>/commitbook-engine-<version>.aar`
- [ ] Add `scripts/fetch-aar.sh` as the analogue of the Apple repo's `fetch-xcframework.sh`, verifies SHA256, places the AAR under `app/libs/`, fails CI when `.core-version` still has placeholder values.

## 2. Rename Kotlin types: `Workspace*` → `CommitBook*`

- [ ] `app/src/main/java/com/commitbook/app/data/model/Models.kt`:
  - `WorkspaceInput` → `CommitBookInput`. Drop `remoteUrl`, `localRoot`. Add `owner`, `repo` (renamed from `repoName`).
  - `WorkspaceSummary` → `CommitBookSummary`. Add `owner`, `repo`, `docCount`, `conflictCount`.
  - Add `DiscoveredCommitBook`, `RepoInfo`, `SyncMode { AiResolve, Manual }`.
  - Add the structured AI conflict request/response types (including nullable error), explicit write/delete action, `ConflictResolutionContinuation`, and nullable ancestor/local/remote fields on `ConflictSummary`.
  - `SyncResultSummary`: add `committed: Boolean`, `conflictsResolved: Int`, `manualConflicts: Int`.

- [ ] `app/src/main/java/com/commitbook/app/data/engine/CommitBookEngine.kt`:
  - Rename all methods `*Workspace` → `*CommitBook`.
  - `initCommitBook(input: CommitBookInput, token: String): CommitBookSummary` - now `suspend` AND takes `token`.
  - Add `validatePAT(token: String): List<RepoInfo>` and `discoverCommitBooks(token: String): List<DiscoveredCommitBook>`.
  - `syncCommitBook(commitBookId: String, mode: SyncMode, token: String): SyncResultSummary`.

## 3. Implement `RealEngine.kt` (mirror of Apple's RealEngine.swift)

- [ ] New file `app/src/main/java/com/commitbook/app/data/engine/RealEngine.kt`. Wraps the UniFFI-generated `CommitBookEngineClient`.
- [ ] Construct `CommitBookEngineClient(workspacesRoot, conflictResolver)` using:
  - `workspacesRoot` = `context.filesDir / "commitbook" / "repos"`.
  - Create both directories on first run.
- [ ] Implement `ConflictResolverCallback` in Kotlin. Its synchronous entry point starts the app's async HTTPS request and returns immediately; complete the supplied continuation with explicit content, deletion, or an error within 120 seconds. It never calls a desktop app. Pass `null` until configured and surface the engine's actionable error if an `AiResolve` sync encounters a conflict without it.
- [ ] Map UniFFI types → app types via small adapters (the Kotlin equivalent of Apple's `FFIMapper.swift`).
- [ ] Map UniFFI errors → app errors (Apple's `FFIErrorAdapter.swift` analogue).

## 4. Update `MockEngine.kt`

- [ ] Add stub implementations for `validatePAT`, `discoverCommitBooks`, `SyncMode` parameter on sync, new fields on `CommitBookSummary`.
- [ ] Keep enough fake behavior that Compose previews and the no-network path stay usable.

## 5. Update `AppContainer` to prefer `RealEngine` when AAR is present

- [ ] In `app/src/main/java/com/commitbook/app/di/AppContainer.kt`: try to instantiate `RealEngine`; fall back to `MockEngine` if the AAR isn't bundled or initialization fails.

## 6. Rename screens / view models

- [ ] `ui/screens/workspace/WorkspaceListScreen.kt` → `CommitBookListScreen.kt`.
- [ ] `ui/screens/workspace/WorkspaceAddScreen.kt` → `CommitBookAddScreen.kt`.
- [ ] `ui/screens/auth/RepoPickerScreen.kt`, wire to `discoverCommitBooks`.
- [ ] `ui/viewmodel/WorkspaceListViewModel.kt` → `CommitBookListViewModel.kt`.
- [ ] Update navigation routes in `ui/navigation/AppNavigation.kt`.
- [ ] Mechanical find/replace `Workspace` → `CommitBook` across the codebase. ~25 files touched.

## 7. Token storage

- [ ] `data/secure/TokenStore.kt` already exists as `EncryptedSharedPreferences`. Confirm it stores the GitHub PAT and that ViewModels read from it before calling sync/discover.
- [ ] PAT entry flow (`PATEntryScreen.kt`): on submit, call `validatePAT(token)`; on success, store via `TokenStore` and navigate to `RepoPickerScreen`.

## 8. Conflict UI updates

- [ ] Pick a default `SyncMode` per code path:
  - Background WorkManager job: `AiResolve`.
  - User-tapped Sync button: `AiResolve` with a banner if `manualConflicts > 0`.
  - From the conflict review screen: `Manual`.
- [ ] `ConflictListScreen.kt` / `ConflictDetailScreen.kt`: render nullable sides and binary/special conflicts; map text actions to `take_local` / `take_remote` / `keep_both` / `manual_edit`, with an absent selected side meaning deletion.

## 9. CI / testing

- [ ] Update `app/src/test/java/com/commitbook/app/MockEngineTest.kt` to exercise the new methods.
- [ ] Add integration tests against the real AAR when a `.core-version` with a real release is present.
- [ ] Smoke test on a physical device: PAT entry → discover → create → edit doc → sync → see commit on GitHub.

## 10. Background sync (deferred)

`WorkManager` integration for periodic sync isn't in the first Android integration scope. Add a separate plan when it's needed. The `auto_sync` preference per CommitBook is already supported on the engine side (`Preferences::auto_sync` in `<clone>/.CommitBook/local/preferences.toml`); the Android side just needs to read it and schedule a worker accordingly.

## Reminders for the engine repo

- [ ] Add `scripts/build-android-aar.sh` analogous to `scripts/build-xcframework.sh` (lives in `crates/commitbook-client/scripts/`).
- [ ] Tag conventions match iOS: `0.5.1`, `0.6.0`, no `v` prefix.
- [ ] Both artifacts (xcframework + AAR) attached to the same release tag, so `.core-version` in both app repos can pin to the same number.

## Local clone identity contract

`CommitBookSummary.commitbookId` is an eight-character local clone identifier,
not `owner/repo`. Persist the value returned by the engine; never reconstruct
it from a path or remote. Resolve the current app-private `workspacesRoot` at
startup, then list clones. Identity is stored in the ignored
`.CommitBook/local/CommitBook-ID.toml` with the key `CommitBook-Id`. Moving the
app storage root or renaming a clone does not change its identity.

For an imported direct-child clone, call `registerLocalCommitbook(relativePath)`
to initialize local identity without Git publication. Listing is read-only;
show broken-clone diagnostics for missing/invalid/duplicate IDs. Do not silently
reset identities. After explicitly removing a copied clone's duplicate identity
file, register that copy again and replace its saved UI selection.

## Provider and storage contract

The constructor takes only `workspacesRoot` and the optional resolver callback;
there is no database path. Map `StorageError` / `storageError` instead of
`DatabaseError` / `databaseError`. Native remote discovery and cloning support
GitHub only; non-GitHub provider input fails before cloning or publication.
Existing non-GitHub clones can use local registration. Display names and sync
remote/branch settings remain configurable and separate from local identity.
