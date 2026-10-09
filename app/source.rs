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

/// How each native provider can be kept in sync, declared like a connector's
/// modes: `mash index` takes a snapshot, and `index --watch` follows file
/// changes. Every provider is a set of files or databases Mash can watch.
pub(crate) const fn native_modes(provider: Provider) -> &'static [crate::connectors::Mode] {
    use crate::connectors::Mode::{Live, Snapshot};
    match provider {
        Provider::ClaudeCode
        | Provider::Codex
        | Provider::Opencode
        | Provider::Copilot
        | Provider::Cursor
        | Provider::Antigravity => &[Snapshot, Live],
    }
}

/// Every native provider's topology, declared like a connector's: a chat is a
/// root container, turns segment it, and messages sit in a turn, or directly
/// in the chat when they belong to none (injected or empty records).
pub(crate) fn native_schema(provider: Provider) -> crate::connectors::Schema {
    use crate::connectors::{Kind, KindRule, Role};
    let kind = |name: &str| {
        Kind::try_from(format!("{}:{name}", provider.as_str()))
            .expect("provider names are valid kind namespaces")
    };
    crate::connectors::Schema {
        kinds: vec![
            KindRule {
                kind: kind("chat"),
                role: Role::Container,
                parents: Vec::new(),
                root: true,
            },
            KindRule {
                kind: kind("turn"),
                role: Role::Segment,
                parents: vec![kind("chat")],
                root: false,
            },
            KindRule {
                kind: kind("message"),
                role: Role::Entry,
                parents: vec![kind("chat"), kind("turn")],
                root: false,
            },
        ],
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

/// A source's declared sync modes, and for connectors when each kind of
/// delivery last committed (Unix milliseconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SourceStatus {
    pub(crate) source: Source,
    /// Empty when a connector has not declared its modes.
    pub(crate) modes: Vec<crate::connectors::Mode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_snapshot_at: Option<i64>,
    /// Only a complete snapshot deletes what the source no longer holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_complete_snapshot_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_live_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_change_at: Option<i64>,
}

impl SourceStatus {
    pub(crate) fn native(provider: Provider) -> Self {
        Self {
            source: Source::Provider(provider),
            modes: native_modes(provider).to_vec(),
            last_snapshot_at: None,
            last_complete_snapshot_at: None,
            last_live_at: None,
            last_change_at: None,
        }
    }
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
    object: crate::connectors::Role,
    role: Option<&crate::role::Role>,
    author: Option<crate::connectors::AuthorKind>,
) -> &str {
    use crate::connectors::AuthorKind;
    match (object, role, author) {
        (crate::connectors::Role::Document, ..) => "document",
        (_, Some(role), _) => role.as_str(),
        (_, None, Some(AuthorKind::Human)) => "human",
        (_, None, Some(AuthorKind::Bot)) => "bot",
        (_, None, Some(AuthorKind::Unknown) | None) => "unknown author",
    }
}
