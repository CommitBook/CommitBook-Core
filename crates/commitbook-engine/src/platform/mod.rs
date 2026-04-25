//! Platform abstraction traits.
//!
//! Desktop git sync uses system Git credentials. File-backed secrets are
//! available here for token-backed transports; mobile builds supply their own
//! implementations across the FFI boundary (iOS Keychain / Android Keystore for
//! secrets; host callback sinks for logs).

pub mod credentials;
pub mod logger;
pub mod secret_store;

pub use credentials::{CredentialProvider, SystemCredentials, TokenCredentials};
pub use logger::{LogLevel, Logger};
pub use secret_store::{FileSecretStore, SecretBundle, SecretStore};
