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

/// Who produced the evidence, for display: a native role or a connector author.
/// One speaker model for both origins. Native entries carry a role, connector
/// entries carry an author kind; retrieval exposes the normalized form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Author {
    /// Source-native identifier: the role string for native entries, the
    /// connector author id where the source supplies one.
    /// Always serialized, so both origins expose the same shape.
    pub id: Option<String>,
    pub kind: crate::connectors::AuthorKind,
}

pub(crate) fn author(
    role: Option<crate::role::Role>,
    kind: Option<crate::connectors::AuthorKind>,
) -> Option<Author> {
    use crate::connectors::AuthorKind;
    match (role, kind) {
        (Some(role), _) => Some(Author {
            kind: match role {
                crate::role::Role::User | crate::role::Role::Developer => AuthorKind::Human,
                crate::role::Role::Assistant => AuthorKind::Bot,
                _ => AuthorKind::Unknown,
            },
            id: Some(match role {
                crate::role::Role::Other(_) => "other".to_owned(),
                role => role.as_str().to_owned(),
            }),
        }),
        (None, Some(kind)) => Some(Author { id: None, kind }),
        (None, None) => None,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::AuthorKind;
    use crate::role::Role;

    /// One speaker shape for native and connector evidence; only `kind` and the
    /// optional source-native `id` differ.
    #[test]
    fn native_and_connector_authors_share_one_shape() {
        let native = author(Some(Role::User), None).expect("native author");
        let bot = author(Some(Role::Assistant), None).expect("assistant author");
        let slack = author(None, Some(AuthorKind::Bot)).expect("connector author");

        assert_eq!(native.kind, AuthorKind::Human);
        assert_eq!(bot.kind, AuthorKind::Bot);
        assert_eq!(slack.kind, AuthorKind::Bot);
        assert_eq!(native.id.as_deref(), Some("user"));
        assert_eq!(bot.id.as_deref(), Some("assistant"));
        assert_eq!(slack.id, None);

        let encode = |value: &Author| serde_json::to_value(value).unwrap();
        let keys = |value: &Author| {
            let mut keys: Vec<_> = encode(value).as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(keys(&native), keys(&slack), "same fields for both origins");
        assert_eq!(encode(&slack)["kind"], "bot");
        assert_eq!(encode(&native)["id"], "user");
    }
}
