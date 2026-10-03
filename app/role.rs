use rmcp::schemars;
use serde::{Deserialize, Serialize};

/// Preserve provider-defined roles while making the common roles explicit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(from = "String", into = "String")]
pub(crate) enum Role {
    User,
    Assistant,
    System,
    Developer,
    Tool,
    Other(String),
}

impl Role {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Developer => "developer",
            Self::Tool => "tool",
            Self::Other(value) => value,
        }
    }
}
impl From<String> for Role {
    fn from(value: String) -> Self {
        match value.as_str() {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "system" => Self::System,
            "developer" => Self::Developer,
            "tool" => Self::Tool,
            _ => Self::Other(value),
        }
    }
}
impl From<&str> for Role {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl From<Role> for String {
    fn from(value: Role) -> Self {
        value.as_str().to_owned()
    }
}
impl rusqlite::types::ToSql for Role {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}
impl rusqlite::types::FromSql for Role {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(value.as_str()?.into())
    }
}

impl schemars::JsonSchema for Role {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Role".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        String::json_schema(generator)
    }
}
