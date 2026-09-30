//! A native tree edge, or the ordinal neighbourhood used by linear histories.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum MessageContext {
    #[default]
    Linear,
    Root,
    Parent {
        ordinal: i64,
    },
}

impl MessageContext {
    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde skip_serializing_if callback takes a reference"
    )]
    pub(crate) const fn is_linear(&self) -> bool {
        matches!(self, Self::Linear)
    }

    pub(crate) const fn parent(self) -> Option<i64> {
        match self {
            Self::Linear => None,
            Self::Root => Some(-1),
            Self::Parent { ordinal } => Some(ordinal),
        }
    }

    pub(crate) const fn valid(self, ordinal: i64) -> bool {
        match self {
            Self::Linear | Self::Root => true,
            Self::Parent { ordinal: parent } => parent >= 0 && parent < ordinal,
        }
    }

    pub(crate) fn from_parent(parent: Option<i64>) -> Result<Self, crate::AppError> {
        match parent {
            None => Ok(Self::Linear),
            Some(-1) => Ok(Self::Root),
            Some(ordinal) if ordinal >= 0 => Ok(Self::Parent { ordinal }),
            _ => Err(crate::AppError::database_data(
                "invalid message context parent",
            )),
        }
    }
}
