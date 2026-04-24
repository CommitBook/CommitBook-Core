//! Platform abstraction traits.
//!
//! Desktop builds use the file-backed implementations in this module
//! (`FileSecretStore`, and the existing `FileLogger` which implements `Logger`).
//! Mobile builds supply their own implementations across the FFI boundary
//! (iOS Keychain / Android Keystore for secrets; host callback sinks for logs).

pub mod credentials;
pub mod logger;
pub mod secret_store;

pub use credentials::{CredentialProvider, SystemCredentials, TokenCredentials};
pub use logger::{LogLevel, Logger};
pub use secret_store::{FileSecretStore, SecretBundle, SecretStore};
