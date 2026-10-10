//! Portable canonical records. No storage-engine schema, vectors or credentials.
use crate::{
    attachments::Metadata, message_context::MessageContext, provider::Provider, role::Role,
};
use serde::{Deserialize, Serialize};

pub(crate) const VERSION: u32 = 1;

/// Archive version 1 keeps its original receipt encoding across releases.
/// Live receipts use `views_ready`; changing the portable wire key would make
/// pre-rename exports impossible to import during the required schema upgrade.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    schema_version: crate::connectors::Version,
    pub(crate) source_id: crate::connectors::Id,
    pub(crate) batch_id: crate::connectors::Id,
    source_revision: u64,
    silver_revision: u64,
    #[serde(rename = "gold_ready")]
    views_ready: bool,
}

impl From<crate::connectors::Receipt> for Receipt {
    fn from(receipt: crate::connectors::Receipt) -> Self {
        Self {
            schema_version: receipt.schema_version,
            source_id: receipt.source_id,
            batch_id: receipt.batch_id,
            source_revision: receipt.source_revision,
            silver_revision: receipt.silver_revision,
            views_ready: receipt.views_ready,
        }
    }
}

impl From<Receipt> for crate::connectors::Receipt {
    fn from(receipt: Receipt) -> Self {
        Self {
            schema_version: receipt.schema_version,
            source_id: receipt.source_id,
            batch_id: receipt.batch_id,
            source_revision: receipt.source_revision,
            silver_revision: receipt.silver_revision,
            views_ready: receipt.views_ready,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Record {
    ConnectorSource {
        source_id: crate::connectors::Id,
        revision: u64,
    },
    ConnectorObject {
        source_id: crate::connectors::Id,
        object: crate::connectors::Object,
    },
    ConnectorSchema {
        source_id: crate::connectors::Id,
        schema: crate::connectors::Schema,
    },
    ConnectorReceipt {
        fingerprint: String,
        receipt: Receipt,
    },
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
    Conversation {
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
        conversation_id: String,
        ordinal: i64,
        role: Role,
        content: String,
        dialogue_text: Option<String>,
        context: MessageContext,
        created_at: Option<i64>,
        fingerprint: String,
    },
    Enrichment {
        conversation_id: String,
        evidence: super::enrichment::Evidence,
    },
    Tombstone {
        provider: Provider,
        conversation_id: String,
        forgotten_at: i64,
    },
    ClaudeSource {
        source_key: String,
        source_path: String,
        conversation_id: String,
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
            Self::ConnectorObject { object, .. } => object
                .validate()
                .map_err(|_| crate::AppError::usage("invalid connector archive object")),
            Self::ConnectorSchema { schema, .. } => schema
                .validate()
                .map_err(|_| crate::AppError::usage("invalid connector archive schema")),
            Self::ConnectorReceipt { fingerprint, .. }
                if fingerprint.len() != 64
                    || !fingerprint.bytes().all(|value| value.is_ascii_hexdigit()) =>
            {
                Err(crate::AppError::usage("invalid connector archive receipt"))
            }
            Self::Message {
                attachments,
                context,
                ordinal,
                ..
            } => {
                if *ordinal < 0 || !context.valid(*ordinal) {
                    return Err(crate::AppError::usage("invalid archive message context"));
                }
                crate::attachments::validate_metadata(attachments)
            }
            Self::ClaudeMessage { attachments, .. } => {
                crate::attachments::validate_metadata(attachments)
            }
            Self::Checkpoint { size_bytes, .. } if *size_bytes < 0 => {
                Err(crate::AppError::usage("invalid archive source size"))
            }
            _ => Ok(()),
        }
    }
}
