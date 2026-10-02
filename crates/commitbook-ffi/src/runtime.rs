//! Shared tokio runtime for the async FFI methods.
//!
//! The UDL exposes `validate_pat` / `discover_commitbooks` / `init_commitbook`
//! / `sync_commitbook` as `[Async]`. UniFFI's UDL scaffolding polls those
//! futures on the foreign thread without entering a tokio runtime, so the
//! `reqwest`, `tokio::spawn`, `Semaphore`, and `spawn_blocking` calls inside
//! them would panic. Each method runs its work on this process-global
//! multi-thread runtime via `runtime().spawn(..).await`, whose `JoinHandle`
//! can be awaited from the foreign poller without an ambient runtime.

use std::sync::OnceLock;
use tokio::runtime::Runtime;

/// The process-global multi-thread tokio runtime.
pub(crate) fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build shared tokio runtime")
    })
}
