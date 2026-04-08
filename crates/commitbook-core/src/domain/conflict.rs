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

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "section_conflict" => Some(Self::SectionConflict),
            "frontmatter_conflict" => Some(Self::FrontmatterConflict),
            "file_conflict" => Some(Self::FileConflict),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStatus {
    Open,
    Resolved,
    Ignored,
}

impl ConflictStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::Ignored => "ignored",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "resolved" => Some(Self::Resolved),
            "ignored" => Some(Self::Ignored),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionType {
    TakeLocal,
    TakeRemote,
    KeepBoth,
    ManualEdit,
}

impl ResolutionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TakeLocal => "take_local",
            Self::TakeRemote => "take_remote",
            Self::KeepBoth => "keep_both",
            Self::ManualEdit => "manual_edit",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "take_local" => Some(Self::TakeLocal),
            "take_remote" => Some(Self::TakeRemote),
            "keep_both" => Some(Self::KeepBoth),
            "manual_edit" => Some(Self::ManualEdit),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub id: String,
    pub workspace_id: String,
    pub path: String,
    pub section_path: Option<String>,
    pub conflict_type: ConflictType,
    pub base_content: Option<String>,
    pub local_content: String,
    pub remote_content: String,
    pub merged_preview: Option<String>,
    pub status: ConflictStatus,
    pub resolution_type: Option<ResolutionType>,
    pub opened_at: String,
    pub resolved_at: Option<String>,
}

impl Conflict {
    pub fn new_id() -> String {
        format!("cf_{}", nanoid::nanoid!(12))
    }
}
