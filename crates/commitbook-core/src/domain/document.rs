use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub workspace_id: String,
    pub path: String,
    pub local_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub checksum: String,
    pub dirty: bool,
    pub deleted: bool,
    pub updated_at: String,
}

impl Document {
    /// Compute SHA-256 checksum of content.
    pub fn compute_checksum(content: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hex::encode(hasher.finalize())
    }
}
