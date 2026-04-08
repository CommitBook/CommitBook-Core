use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    GithubApp,
    Pat,
    Ssh,
    ExistingLocalRepo,
}

impl WorkspaceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GithubApp => "github_app",
            Self::Pat => "pat",
            Self::Ssh => "ssh",
            Self::ExistingLocalRepo => "existing_local_repo",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "github_app" => Some(Self::GithubApp),
            "pat" => Some(Self::Pat),
            "ssh" => Some(Self::Ssh),
            "existing_local_repo" => Some(Self::ExistingLocalRepo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Github,
    Gitlab,
    Codeberg,
    GenericGit,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Codeberg => "codeberg",
            Self::GenericGit => "generic_git",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "github" => Some(Self::Github),
            "gitlab" => Some(Self::Gitlab),
            "codeberg" => Some(Self::Codeberg),
            "generic_git" => Some(Self::GenericGit),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalMode {
    Sandbox,
    Folder,
}

impl LocalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sandbox => "sandbox",
            Self::Folder => "folder",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sandbox" => Some(Self::Sandbox),
            "folder" => Some(Self::Folder),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub mode: WorkspaceMode,
    pub provider: Provider,
    pub remote_url: Option<String>,
    pub owner: Option<String>,
    pub repo_name: Option<String>,
    pub branch: String,
    pub local_mode: LocalMode,
    pub local_root: String,
    pub merge_mode: String,
    pub sync_interval_seconds: u64,
    pub auto_sync: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Workspace {
    pub fn new_id() -> String {
        format!("wk_{}", nanoid::nanoid!(12))
    }
}

/// Detects which mode a repo path is currently operating in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoMode {
    /// Managed by the auto-commit scheduler (.CommitBook/config.toml exists)
    AutoCommit,
    /// Managed as a sync workspace (registered in SQLite)
    Workspace,
    /// Not managed by CommitBook
    None,
}
