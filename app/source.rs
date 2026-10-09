//! Where indexed evidence comes from: a native history parser or an external connector.
use crate::provider::Provider;
use rmcp::schemars;
use serde::{Deserialize, Serialize};

/// Serialized as `{"provider": "codex"}` or `{"connector": "<source_id>"}`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Source {
    Provider(Provider),
    /// A connector's `source_id`; native providers never use this namespace.
    Connector(String),
}

impl Source {
    pub(crate) const fn provider(&self) -> Option<Provider> {
        match self {
            Self::Provider(provider) => Some(*provider),
            Self::Connector(_) => None,
        }
    }
}

/// Connector documents stand alone; every native hit is a message.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    #[default]
    Message,
    Document,
}

impl std::fmt::Display for Source {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Provider(provider) => formatter.write_str(provider.as_str()),
            Self::Connector(source) => write!(formatter, "connector {source}"),
        }
    }
}

/// Who produced the evidence, for display: a native role or a connector author.
pub(crate) fn speaker(
    kind: Kind,
    role: Option<&crate::role::Role>,
    author: Option<crate::connectors::AuthorKind>,
) -> &str {
    use crate::connectors::AuthorKind;
    match (kind, role, author) {
        (Kind::Document, ..) => "document",
        (_, Some(role), _) => role.as_str(),
        (_, None, Some(AuthorKind::Human)) => "human",
        (_, None, Some(AuthorKind::Bot)) => "bot",
        (_, None, Some(AuthorKind::Unknown) | None) => "unknown author",
    }
}
