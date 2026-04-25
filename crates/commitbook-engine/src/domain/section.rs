use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A single section in a parsed markdown document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    /// Hierarchical path, e.g. "/Product/Notes" or "/Notes[2]" for duplicates.
    pub path: String,
    /// Raw heading text without the `#` markers.
    pub heading_text: String,
    /// Heading level (1-6).
    pub level: u8,
    /// Body text under this heading (excluding child sections).
    pub content: String,
    /// SHA-256 hash of `content`.
    pub content_hash: String,
    /// Disambiguation ordinal for duplicate sibling headings (None if unique).
    pub ordinal: Option<u32>,
    /// Original verbatim source text (heading line + content block).
    /// Set by the parser; None for programmatically-created sections.
    #[serde(skip)]
    pub raw_source: Option<String>,
}

impl PartialEq for Section {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.heading_text == other.heading_text
            && self.level == other.level
            && self.content == other.content
            && self.content_hash == other.content_hash
            && self.ordinal == other.ordinal
    }
}

impl Eq for Section {}

impl Section {
    pub fn compute_hash(content: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// The complete parsed structure of a markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionTree {
    /// Optional frontmatter block (YAML or TOML).
    pub frontmatter: Option<Frontmatter>,
    /// Content before the first heading.
    pub preamble: String,
    /// Flattened, document-order list of sections.
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontmatterFormat {
    Yaml,
    Toml,
}

/// Parsed frontmatter from a markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    pub format: FrontmatterFormat,
    /// Raw frontmatter text (without delimiters).
    pub raw: String,
    /// Parsed key-value pairs (flat, string values only for V1).
    pub fields: BTreeMap<String, String>,
}
