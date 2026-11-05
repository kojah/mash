//! Agent history result metadata.
use crate::provider::Provider;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

/// What a result is in the object model: its core role and its source's label.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct ObjectType {
    pub(crate) role: crate::connectors::Role,
    pub(crate) kind: String,
}

impl ObjectType {
    pub(crate) fn native_message(provider: Provider) -> Self {
        Self {
            role: crate::connectors::Role::Entry,
            kind: format!("{}:message", provider.as_str()),
        }
    }

    pub(crate) fn native_chat(provider: Provider) -> Self {
        Self {
            role: crate::connectors::Role::Container,
            kind: format!("{}:chat", provider.as_str()),
        }
    }
}

/// A native chat's ancestry for Qdrant points, which do not record turns;
/// the shared server replaces it from shared silver.
pub(crate) fn native_ancestors(
    provider: Provider,
    chat_id: &str,
    title: Option<&str>,
) -> Vec<Ancestor> {
    vec![Ancestor {
        id: chat_id.to_owned(),
        role: crate::connectors::Role::Container,
        kind: format!("{}:chat", provider.as_str()),
        title: title.map(str::to_owned),
    }]
}

/// An enclosing object of a result, listed nearest first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct Ancestor {
    pub(crate) id: String,
    pub(crate) role: crate::connectors::Role,
    pub(crate) kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
}
