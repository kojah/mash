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
    Opencode,
    Copilot,
    Cursor,
    Antigravity,
}

/// Enum keys keep configuration and routing exhaustive without duplicating a
/// field for every provider. The serialized keys remain provider names.
pub(crate) type ProviderRoots = std::collections::BTreeMap<Provider, Vec<std::path::PathBuf>>;

impl Provider {
    pub(crate) const ALL: &[Self] = &[
        Self::ClaudeCode,
        Self::Codex,
        Self::Opencode,
        Self::Copilot,
        Self::Cursor,
        Self::Antigravity,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::Antigravity => "antigravity",
        }
    }

    /// Stable storage identifiers; never renumber existing providers.
    pub(crate) const fn storage_code(self) -> u8 {
        match self {
            Self::ClaudeCode => 1,
            Self::Codex => 2,
            Self::Opencode => 5,
            Self::Copilot => 8,
            Self::Cursor => 11,
            Self::Antigravity => 13,
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
            "opencode" => Ok(Self::Opencode),
            "copilot" => Ok(Self::Copilot),
            "cursor" => Ok(Self::Cursor),
            "antigravity" => Ok(Self::Antigravity),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_providers_keep_stable_storage_codes_and_round_trip() {
        use clap::ValueEnum;
        let expected = [
            (Provider::ClaudeCode, "claude-code", 1),
            (Provider::Codex, "codex", 2),
            (Provider::Opencode, "opencode", 5),
            (Provider::Copilot, "copilot", 8),
            (Provider::Cursor, "cursor", 11),
            (Provider::Antigravity, "antigravity", 13),
        ];
        assert_eq!(Provider::ALL, Provider::value_variants());
        assert_eq!(Provider::ALL.len(), expected.len());
        for (provider, name, code) in expected {
            assert_eq!(provider.as_str(), name);
            assert_eq!(provider.storage_code(), code);
            assert_eq!(name.parse::<Provider>().unwrap(), provider);
            let json = serde_json::to_string(&provider).unwrap();
            assert_eq!(serde_json::from_str::<Provider>(&json).unwrap(), provider);
        }
    }

    #[test]
    fn retired_providers_are_rejected_at_typed_input_boundaries() {
        for name in [
            "gemini-cli",
            "windsurf",
            "cline",
            "aider",
            "hermes",
            "openclaw",
            "pi",
        ] {
            assert!(name.parse::<Provider>().is_err());
            let json = serde_json::to_string(name).unwrap();
            assert!(serde_json::from_str::<Provider>(&json).is_err());
            assert!(
                serde_json::from_str::<crate::search_filter::Filter>(&format!(
                    r#"{{"provider":[{json}]}}"#
                ))
                .is_err()
            );
        }
    }
}
