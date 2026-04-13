use anyhow::{bail, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::domain::transport::{
    RemoteDocument, RemoteTransport, RepoDescriptor, WriteFileInput, WriteFileResult,
};

/// A configurable mock transport for testing sync planner and pipeline.
pub struct MockTransport {
    files: HashMap<String, String>,
    head: String,
    written: Mutex<Vec<WriteFileInput>>,
    fail_writes: bool,
}

impl MockTransport {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            head: "mock_head_sha_000".to_string(),
            written: Mutex::new(Vec::new()),
            fail_writes: false,
        }
    }

    pub fn with_file(mut self, path: &str, content: &str) -> Self {
        self.files.insert(path.to_string(), content.to_string());
        self
    }

    pub fn with_head(mut self, head: &str) -> Self {
        self.head = head.to_string();
        self
    }

    pub fn with_fail_writes(mut self) -> Self {
        self.fail_writes = true;
        self
    }

    /// Returns files that were written via `write_files`.
    pub fn written_files(&self) -> Vec<WriteFileInput> {
        self.written.lock().unwrap().clone()
    }
}

#[async_trait]
impl RemoteTransport for MockTransport {
    async fn validate(&self) -> Result<()> {
        Ok(())
    }

    async fn list_repos(&self) -> Result<Vec<RepoDescriptor>> {
        Ok(Vec::new())
    }

    async fn list_files(&self, _branch: &str) -> Result<Vec<String>> {
        Ok(self.files.keys().cloned().collect())
    }

    async fn read_file(&self, _branch: &str, path: &str) -> Result<RemoteDocument> {
        match self.files.get(path) {
            Some(content) => Ok(RemoteDocument {
                path: path.to_string(),
                content: content.clone(),
                revision: format!("rev_{}", path.replace('/', "_")),
            }),
            None => bail!("File not found: {}", path),
        }
    }

    async fn write_files(
        &self,
        _branch: &str,
        inputs: Vec<WriteFileInput>,
    ) -> Result<Vec<WriteFileResult>> {
        if self.fail_writes {
            bail!("Simulated push failure");
        }
        let mut written = self.written.lock().unwrap();
        let results = inputs
            .iter()
            .map(|input| WriteFileResult {
                path: input.path.clone(),
                new_revision: format!("new_rev_{}", input.path.replace('/', "_")),
            })
            .collect();
        written.extend(inputs);
        Ok(results)
    }

    async fn delete_file(
        &self,
        _branch: &str,
        _path: &str,
        _message: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_head(&self, _branch: &str) -> Result<String> {
        Ok(self.head.clone())
    }
}
