use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize,
    clap::ValueEnum,
    rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Provider {
    ClaudeCode,
    Codex,
    Aider,
    Hermes,
    Opencode,
    Openclaw,
    Pi,
    Copilot,
    GeminiCli,
    Cline,
    Cursor,
    Windsurf,
}

/// Enum keys keep configuration and routing exhaustive without duplicating a
/// field for every provider. The serialized keys remain provider names.
pub(crate) type ProviderRoots = std::collections::BTreeMap<Provider, Vec<std::path::PathBuf>>;

impl Provider {
    pub(crate) const ALL: &[Self] = &[
        Self::ClaudeCode,
        Self::Codex,
        Self::Aider,
        Self::Hermes,
        Self::Opencode,
        Self::Openclaw,
        Self::Pi,
        Self::Copilot,
        Self::GeminiCli,
        Self::Cline,
        Self::Cursor,
        Self::Windsurf,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Aider => "aider",
            Self::Hermes => "hermes",
            Self::Opencode => "opencode",
            Self::Openclaw => "openclaw",
            Self::Pi => "pi",
            Self::Copilot => "copilot",
            Self::GeminiCli => "gemini-cli",
            Self::Cline => "cline",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
        }
    }

    /// Stable storage identifiers; never renumber existing providers.
    pub(crate) const fn storage_code(self) -> u8 {
        match self {
            Self::ClaudeCode => 1,
            Self::Codex => 2,
            Self::Aider => 3,
            Self::Hermes => 4,
            Self::Opencode => 5,
            Self::Openclaw => 6,
            Self::Pi => 7,
            Self::Copilot => 8,
            Self::GeminiCli => 9,
            Self::Cline => 10,
            Self::Cursor => 11,
            Self::Windsurf => 12,
        }
    }

    pub(crate) const fn redis_tag(self) -> &'static str {
        match self {
            Self::ClaudeCode => "c",
            Self::Codex => "x",
            Self::Aider => "a",
            Self::Hermes => "h",
            Self::Opencode => "o",
            Self::Openclaw => "l",
            Self::Pi => "p",
            Self::Copilot => "g",
            Self::GeminiCli => "m",
            Self::Cline => "n",
            Self::Cursor => "u",
            Self::Windsurf => "w",
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for Provider {
    type Err = crate::AppError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude-code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "aider" => Ok(Self::Aider),
            "hermes" => Ok(Self::Hermes),
            "opencode" => Ok(Self::Opencode),
            "openclaw" => Ok(Self::Openclaw),
            "pi" => Ok(Self::Pi),
            "copilot" => Ok(Self::Copilot),
            "gemini-cli" => Ok(Self::GeminiCli),
            "cline" => Ok(Self::Cline),
            "cursor" => Ok(Self::Cursor),
            "windsurf" => Ok(Self::Windsurf),
            _ => Err(crate::AppError::usage("unsupported provider")),
        }
    }
}

impl rusqlite::types::ToSql for Provider {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl rusqlite::types::FromSql for Provider {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map_err(|_| rusqlite::types::FromSqlError::InvalidType)
    }
}

impl<'a> tokio_postgres::types::FromSql<'a> for Provider {
    fn from_sql(
        ty: &tokio_postgres::types::Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let value = <&str as tokio_postgres::types::FromSql>::from_sql(ty, raw)?;
        value
            .parse()
            .map_err(|_| "unsupported stored provider".into())
    }
    fn accepts(ty: &tokio_postgres::types::Type) -> bool {
        <&str as tokio_postgres::types::FromSql>::accepts(ty)
    }
}
