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
}

/// Enum keys keep configuration and routing exhaustive without duplicating a
/// field for every provider. The serialized keys remain provider names.
pub(crate) type ProviderRoots = std::collections::BTreeMap<Provider, Vec<std::path::PathBuf>>;

impl Provider {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
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
