use super::*;

/// Guard for the shared-runtime wiring behind the four `[Async]` FFI methods.
///
/// UniFFI's UDL scaffolding polls `[Async]` methods on the foreign caller's
/// thread with no ambient tokio runtime, so the `reqwest` / `tokio::spawn` /
/// `spawn_blocking` calls inside these methods used to panic. Each method now
/// runs its body on the process-global runtime in `crate::runtime`.
///
/// `pollster::block_on` drives the future from a plain thread with no tokio
/// runtime installed, which is exactly that condition. `sync_commitbook` is
/// the method used here because it fails fast at the registry lookup in
/// `sync_ops::sync_one_commitbook` (filesystem only, no network) while still
/// exercising the full `runtime().spawn(..)` + `spawn_blocking` path.
#[test]
fn async_ffi_methods_run_without_ambient_tokio_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let client = CommitBookEngineClient::new(
        "unused-db-path".to_string(),
        tmp.path().to_string_lossy().into_owned(),
    )
    .unwrap();

    // A bogus id resolves to NotFound before any network call is attempted.
    // Pre-fix this line panicked instead of returning.
    let err = pollster::block_on(client.sync_commitbook(
        "does-not-exist".to_string(),
        SyncMode::Manual,
        "token".to_string(),
    ))
    .expect_err("a bogus commitbook id should error, not succeed");

    assert!(
        matches!(err, CommitBookError::NotFound { .. }),
        "expected NotFound, got: {err}"
    );

    // The shared runtime is a process-global OnceLock, so a second call must
    // reuse it rather than panic or deadlock.
    let second = pollster::block_on(client.sync_commitbook(
        "also-missing".to_string(),
        SyncMode::Manual,
        "token".to_string(),
    ));
    assert!(
        matches!(second, Err(CommitBookError::NotFound { .. })),
        "second call should also return NotFound, got: {second:?}"
    );
}
