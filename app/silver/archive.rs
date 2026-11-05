//! Portable canonical records. No storage-engine schema, vectors or credentials.
use crate::{attachments::Metadata, provider::Provider, role::Role};
use serde::{Deserialize, Serialize};

pub(crate) const VERSION: u32 = 2;

/// Archive record schema 2 uses chat terminology. Ancestry is expressed in
/// positions within the frozen chat, independently of live message IDs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Context {
    #[default]
    Linear,
    Root,
    Parent {
        ordinal: i64,
    },
}

impl Context {
    pub(crate) const fn parent(self) -> Option<i64> {
        match self {
            Self::Linear => None,
            Self::Root => Some(-1),
            Self::Parent { ordinal } => Some(ordinal),
        }
    }

    const fn valid(self, ordinal: i64) -> bool {
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
                "invalid archive context parent",
            )),
        }
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Record {
    /// The node's shared-publication identity, restored only by an upgrade
    /// import that retires the exporting database.
    Publication {
        instance: String,
        revision: i64,
    },
    State {
        revision: u64,
        interpretation_version: u32,
    },
    Chat {
        id: String,
        provider: Provider,
        source_path: String,
        source_key: String,
        source_session: Option<String>,
        title: Option<String>,
        created_at: Option<i64>,
        updated_at: Option<i64>,
        source_fingerprint: String,
        project: Option<String>,
        repository: Option<String>,
    },
    Message {
        attachments: Vec<Metadata>,
        id: String,
        chat_id: String,
        ordinal: i64,
        role: Role,
        content: String,
        dialogue_text: Option<String>,
        context: Context,
        created_at: Option<i64>,
        fingerprint: String,
        /// Absent from archives written before tool activity was recorded.
        #[serde(default, skip_serializing_if = "crate::tools::Activity::is_empty")]
        tools: crate::tools::Activity,
    },
    Enrichment {
        chat_id: String,
        evidence: super::enrichment::Evidence,
    },
    Tombstone {
        provider: Provider,
        chat_id: String,
        forgotten_at: i64,
    },
    ClaudeSource {
        source_key: String,
        source_path: String,
        chat_id: String,
        title: Option<String>,
        project: Option<String>,
        repository: Option<String>,
    },
    ClaudeMessage {
        attachments: Vec<Metadata>,
        source_key: String,
        message_id: String,
        parent_id: Option<String>,
        role: Role,
        content: String,
        dialogue_text: Option<String>,
        created_at: Option<i64>,
        #[serde(default, skip_serializing_if = "crate::tools::Activity::is_empty")]
        tools: crate::tools::Activity,
    },
    Checkpoint {
        provider: Provider,
        source_path: String,
        source_key: String,
        size_bytes: i64,
        modified_ns: i64,
        fingerprint: String,
        interpretation_version: u32,
    },
    Scan {
        provider: Provider,
        interpretation_version: u32,
        state: ScanState,
    },
    Rejection {
        provider: Provider,
        source_key: String,
        source_path: String,
        modified_ns: i64,
    },
    AttachmentPolicy {
        fingerprint: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScanState {
    Pending,
    Complete,
    Incomplete,
}

impl Record {
    pub(crate) fn validate(&self) -> Result<(), crate::AppError> {
        match self {
            Self::Message {
                attachments,
                context,
                ordinal,
                tools,
                ..
            } => {
                if *ordinal < 0 || !context.valid(*ordinal) {
                    return Err(crate::AppError::usage("invalid archive message context"));
                }
                tools.validate()?;
                crate::attachments::validate_metadata(attachments)
            }
            Self::ClaudeMessage {
                attachments, tools, ..
            } => {
                tools.validate()?;
                crate::attachments::validate_metadata(attachments)
            }
            Self::Checkpoint { size_bytes, .. } if *size_bytes < 0 => {
                Err(crate::AppError::usage("invalid archive source size"))
            }
            _ => Ok(()),
        }
    }
}
