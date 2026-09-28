//! Enumerated `config.toml` values. Each one parses from and prints as the
//! exact lowercase string users write in the file, and an unknown value
//! names every accepted alternative.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

macro_rules! config_enum {
    ($(#[$meta:meta])* $name:ident, $label:literal { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = anyhow::Error;

            fn from_str(input: &str) -> Result<Self> {
                let input = input.trim();
                Self::ALL
                    .iter()
                    .copied()
                    .find(|value| value.as_str() == input)
                    .ok_or_else(|| {
                        let allowed: Vec<&str> = Self::ALL.iter().map(|v| v.as_str()).collect();
                        anyhow!(
                            "Unknown {} `{}`; expected one of: {}",
                            $label,
                            input,
                            allowed.join(", ")
                        )
                    })
            }
        }
    };
}

config_enum!(
    /// How commit messages are written (`[commit] mode`).
    CommitMode, "commit mode" {
        Timestamp => "timestamp",
        Ai => "ai",
    }
);

config_enum!(
    /// Which AI CLI writes commit messages (`[commit] agent`). `Any` tries
    /// every installed agent in a fixed order.
    CommitAgent, "commit agent" {
        Any => "any",
        Claude => "claude",
        Codex => "codex",
        Copilot => "copilot",
        Gemini => "gemini",
        Cursor => "cursor",
    }
);

config_enum!(
    /// What happens when a merge leaves conflicts (`[conflicts] mode`).
    ConflictMode, "conflict mode" {
        Both => "both",
        Manual => "manual",
        Ai => "ai",
        Review => "review",
    }
);

config_enum!(
    /// AI CLI used by the `ai` and `review` conflict modes.
    Agent, "agent" {
        Claude => "claude",
        Codex => "codex",
        Copilot => "copilot",
        Gemini => "gemini",
        Cursor => "cursor",
    }
);

config_enum!(
    /// How a device authenticates with its remote. Informational on desktop,
    /// which always uses the user's normal Git credentials.
    Auth, "auth" {
        GithubApp => "github_app",
        Pat => "pat",
        Ssh => "ssh",
        ExistingLocalRepo => "existing_local_repo",
    }
);

impl Agent {
    /// Key of this agent's commit-message provider in `ProviderChain`.
    pub fn commit_provider_key(self) -> &'static str {
        match self {
            Self::Claude => "claude-cli",
            Self::Codex => "codex-cli",
            Self::Copilot => "gh-copilot",
            Self::Gemini => "gemini-cli",
            Self::Cursor => "cursor-agent",
        }
    }
}

impl CommitAgent {
    /// The single agent this setting names, or `None` for `any`.
    pub fn agent(self) -> Option<Agent> {
        match self {
            Self::Any => None,
            Self::Claude => Some(Agent::Claude),
            Self::Codex => Some(Agent::Codex),
            Self::Copilot => Some(Agent::Copilot),
            Self::Gemini => Some(Agent::Gemini),
            Self::Cursor => Some(Agent::Cursor),
        }
    }
}

/// Order in which `[commit] agent = "any"` tries installed agents.
pub const ANY_AGENT_ORDER: &[Agent] = &[
    Agent::Copilot,
    Agent::Claude,
    Agent::Codex,
    Agent::Gemini,
    Agent::Cursor,
];

/// Longest retention `[logs] keep` accepts, in days.
pub const MAX_LOG_KEEP_DAYS: u32 = 3650;

/// Log retention (`[logs] keep`): `"<N>d"` or `"forever"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum LogKeep {
    Days(u32),
    Forever,
}

impl LogKeep {
    /// Days to keep, or `None` to keep logs forever.
    pub fn days(self) -> Option<u32> {
        match self {
            Self::Days(days) => Some(days),
            Self::Forever => None,
        }
    }
}

impl Default for LogKeep {
    fn default() -> Self {
        Self::Days(30)
    }
}

impl fmt::Display for LogKeep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Days(days) => write!(f, "{days}d"),
            Self::Forever => f.write_str("forever"),
        }
    }
}

impl FromStr for LogKeep {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self> {
        let input = input.trim();
        if input == "forever" {
            return Ok(Self::Forever);
        }
        let days = input
            .strip_suffix('d')
            .and_then(|days| days.parse::<u32>().ok())
            .ok_or_else(|| {
                anyhow!("Invalid log retention `{input}`; use `<N>d` (e.g. `30d`) or `forever`")
            })?;
        if !(1..=MAX_LOG_KEEP_DAYS).contains(&days) {
            bail!("Log retention must be between 1d and {MAX_LOG_KEEP_DAYS}d, or `forever`");
        }
        Ok(Self::Days(days))
    }
}

impl TryFrom<String> for LogKeep {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}

impl From<LogKeep> for String {
    fn from(value: LogKeep) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
#[path = "values_tests.rs"]
mod tests;
