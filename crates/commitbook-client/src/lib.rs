//! `commitbook-client` — UniFFI SDK exposing `CommitBookEngineClient` to
//! native iOS/macOS/Android apps. Wraps `commitbook-engine` orchestration.
//!
//! UDL spec: `crates/commitbook-client/src/commitbook.udl`.

mod auth;
mod client;
mod commitbooks_ops;
mod conflicts;
mod documents;
mod errors;
mod sync_ops;
mod types;

pub use client::*;
pub use errors::*;
pub use types::*;

uniffi::include_scaffolding!("commitbook");
