//! Stable native ancestry, independent of a message's current display position.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MessageContext {
    #[default]
    Linear,
    Root,
    Parent {
        id: String,
    },
}

impl MessageContext {
    pub const fn is_linear(&self) -> bool {
        matches!(self, Self::Linear)
    }

    /// SQL uses NULL for linear ancestry and the empty string for a tree root.
    /// Message IDs are nonempty, so neither can alias a parent identity.
    pub fn parent(&self) -> Option<&str> {
        match self {
            Self::Linear => None,
            Self::Root => Some(""),
            Self::Parent { id } => Some(id),
        }
    }

    pub fn valid(&self, message_id: &str) -> bool {
        match self {
            Self::Linear | Self::Root => true,
            Self::Parent { id } => !id.is_empty() && id != message_id,
        }
    }

    pub fn from_parent(parent: Option<String>) -> Self {
        match parent {
            None => Self::Linear,
            Some(id) if id.is_empty() => Self::Root,
            Some(id) => Self::Parent { id },
        }
    }
}
