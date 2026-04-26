use thiserror::Error;

/// Errors surfaced across the UniFFI boundary. Each variant maps to one of
/// the Apple/Android engine-protocol error cases.
#[derive(Debug, Error)]
pub enum CommitBookError {
    #[error("Database error: {message}")]
    DatabaseError { message: String },
    #[error("Transport error: {message}")]
    TransportError { message: String },
    #[error("Merge error: {message}")]
    MergeError { message: String },
    #[error("Auth error: {message}")]
    AuthError { message: String },
    #[error("Not found: {message}")]
    NotFound { message: String },
    #[error("Invalid input: {message}")]
    InvalidInput { message: String },
}

impl CommitBookError {
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: msg.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound {
            message: msg.into(),
        }
    }

    pub fn database(msg: impl Into<String>) -> Self {
        Self::DatabaseError {
            message: msg.into(),
        }
    }

    pub fn transport(msg: impl Into<String>) -> Self {
        Self::TransportError {
            message: msg.into(),
        }
    }

    pub fn merge(msg: impl Into<String>) -> Self {
        Self::MergeError {
            message: msg.into(),
        }
    }

    pub fn auth(msg: impl Into<String>) -> Self {
        Self::AuthError {
            message: msg.into(),
        }
    }
}

impl From<anyhow::Error> for CommitBookError {
    fn from(e: anyhow::Error) -> Self {
        Self::DatabaseError {
            message: format!("{e:#}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, CommitBookError>;
