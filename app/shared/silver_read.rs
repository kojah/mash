//! Retained shared-silver wire format for rebuilding gold without bronze.
use super::protocol::ConversationRecord;
use super::silver_revision;
use crate::{message_context::MessageContext, role::Role};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Retained {
    pub(super) build: crate::gold::config::Build,
    pub(super) publication_revision: i64,
    pub(super) silver: silver_revision::State,
    pub(super) generation: String,
    pub(super) dimensions: usize,
    pub(super) history: Silver,
}

/// Header fields written before the streamed `history` object.
#[derive(Serialize)]
pub(super) struct Header<'a> {
    pub(super) build: &'a crate::gold::config::Build,
    pub(super) publication_revision: i64,
    pub(super) silver: &'a silver_revision::State,
    pub(super) generation: &'a str,
    pub(super) dimensions: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Silver {
    pub(super) conversations: BTreeMap<String, ConversationRecord>,
    pub(super) messages: BTreeMap<String, Message>,
    #[serde(rename = "tombstones")]
    _tombstones: serde::de::IgnoredAny,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Message {
    #[serde(default, rename = "attachments")]
    _attachments: serde::de::IgnoredAny,
    pub(super) id: String,
    pub(super) conversation_id: String,
    pub(super) ordinal: i64,
    #[serde(default)]
    pub(super) context: MessageContext,
    pub(super) role: Role,
    #[serde(rename = "content")]
    _content: serde::de::IgnoredAny,
    pub(super) projection: String,
    #[serde(rename = "created_at")]
    _created_at: serde::de::IgnoredAny,
}
