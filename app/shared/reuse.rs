//! Publication plans and the silver dependency fingerprint of each thread.
use super::protocol::{ConversationRecord, MessageRecord};
use crate::{AppError, gold::config::Build, message_context::MessageContext, role::Role};
use serde::Serialize;
use std::io::Write;

/// What a node publishes against: the group's gold settings and the node's
/// accepted head, both read from the group's freshness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub(super) build: Build,
    pub(super) generation: String,
    pub(super) dimensions: usize,
    pub(super) instance: Option<String>,
    pub(super) publication_revision: i64,
}

impl Target {
    pub(super) fn from_freshness(
        freshness: &super::Freshness,
        config: &crate::config::SharedConfig,
    ) -> Result<Self, AppError> {
        let group = freshness
            .group_gold
            .as_ref()
            .filter(|_| freshness.group_id == config.group_id)
            .ok_or_else(|| AppError::shared("shared group gold settings are unavailable"))?;
        let node = freshness
            .nodes
            .iter()
            .find(|node| node.node_id == config.node_id);
        Ok(Self {
            build: group.build.clone(),
            generation: group.generation.clone(),
            dimensions: group.dimensions,
            instance: node.map(|node| node.instance.clone()),
            publication_revision: node.map_or(0, |node| node.revision),
        })
    }
}

/// One thread's ordered canonical dependency stream. No vectors, model identity,
/// checkpoint stamps or retrieval policy enter this digest.
pub(super) struct Fingerprint {
    digest: blake3::Hasher,
    conversation_id: String,
    ordinal: Option<i64>,
}

struct Digest<'a>(&'a mut blake3::Hasher);
impl Write for Digest<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Fingerprint {
    pub(super) fn new(conversation: &ConversationRecord) -> Result<Self, AppError> {
        let mut digest = blake3::Hasher::new();
        digest.update(b"mash-shared-thread-dependencies-v1\0");
        serde_json::to_writer(Digest(&mut digest), conversation)
            .map_err(|_| AppError::internal("fingerprint shared thread"))?;
        digest.update(b"\n");
        Ok(Self {
            digest,
            conversation_id: conversation.id.clone(),
            ordinal: None,
        })
    }

    pub(super) fn push(&mut self, message: &MessageRecord) -> Result<(), AppError> {
        if message.conversation_id != self.conversation_id
            || message.ordinal < 0
            || self
                .ordinal
                .is_some_and(|previous| previous >= message.ordinal)
        {
            return Err(AppError::shared("invalid ordered thread dependencies"));
        }
        serde_json::to_writer(Digest(&mut self.digest), &message.silver_ref())
            .map_err(|_| AppError::internal("fingerprint shared message"))?;
        self.digest.update(b"\n");
        self.ordinal = Some(message.ordinal);
        Ok(())
    }

    pub(super) fn finish(self) -> String {
        self.digest.finalize().to_hex().to_string()
    }
}

#[derive(Serialize)]
pub(in crate::shared) struct MessageRef<'a> {
    #[serde(skip_serializing_if = "<[crate::attachments::Metadata]>::is_empty")]
    attachments: &'a [crate::attachments::Metadata],
    id: &'a str,
    conversation_id: &'a str,
    ordinal: i64,
    #[serde(skip_serializing_if = "MessageContext::is_linear")]
    context: MessageContext,
    role: &'a Role,
    content: &'a str,
    projection: &'a str,
    created_at: Option<i64>,
}

impl MessageRecord {
    /// Canonical fields only; vectors and model identity are never silver.
    pub(in crate::shared) fn silver_ref(&self) -> MessageRef<'_> {
        MessageRef {
            attachments: &self.attachments,
            id: &self.id,
            conversation_id: &self.conversation_id,
            ordinal: self.ordinal,
            context: self.context,
            role: &self.role,
            content: &self.content,
            projection: &self.projection,
            created_at: self.created_at,
        }
    }
}

#[cfg(test)]
mod tests;
