use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    SectionConflict,
    FrontmatterConflict,
    FileConflict,
}

impl ConflictType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SectionConflict => "section_conflict",
            Self::FrontmatterConflict => "frontmatter_conflict",
            Self::FileConflict => "file_conflict",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "section_conflict" => Some(Self::SectionConflict),
            "frontmatter_conflict" => Some(Self::FrontmatterConflict),
            "file_conflict" => Some(Self::FileConflict),
            _ => None,
        }
    }
}
