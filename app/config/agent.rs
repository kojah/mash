//! The agent CLIs Mash can register itself with, and the commands that do it.
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "SetupAgent")]
pub enum Agent {
    Codex,
    ClaudeCode,
}

impl Agent {
    /// The `--agent` value and JSON name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
        }
    }

    /// The agent's own command-line program.
    pub const fn program(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
        }
    }

    /// The agent CLI command that removes Mash's user-level MCP registration.
    pub const fn remove_mcp(self) -> &'static str {
        match self {
            Self::Codex => "codex mcp remove mash",
            Self::ClaudeCode => "claude mcp remove --scope user mash",
        }
    }

    /// The agent CLI command that registers `executable` as Mash's MCP server.
    pub fn add_mcp(self, executable: &str) -> String {
        match self {
            Self::Codex => format!("codex mcp add mash -- {executable} mcp"),
            Self::ClaudeCode => format!("claude mcp add --scope user mash -- {executable} mcp"),
        }
    }

    /// The online commands that install and enable the mash@mash plugin.
    pub const fn install_plugin(self) -> &'static str {
        match self {
            Self::ClaudeCode => {
                "claude plugin marketplace add kojah/mash && claude plugin install mash@mash"
            }
            Self::Codex => "codex plugin marketplace add kojah/mash && codex plugin add mash@mash",
        }
    }

    /// The agent CLI command that removes the mash@mash plugin.
    pub const fn remove_plugin(self) -> &'static str {
        match self {
            Self::Codex => "codex plugin remove mash@mash",
            Self::ClaudeCode => "claude plugin uninstall mash@mash",
        }
    }
}
