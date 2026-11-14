//! Publication plans and the silver dependency fingerprint of each thread.
use super::protocol::{ChatRecord, MessageRecord};
use crate::{AppError, message_context::MessageContext, role::Role, views::config::Build};
use serde::Serialize;
use std::io::Write;

/// What a node publishes against: the group's views settings and the node's
/// accepted head, both read from the group's freshness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub(super) build: Build,
    pub(super) generation: String,
    pub(super) dimensions: usize,
    pub(super) instance: Option<String>,
    pub(super) publication_revision: i64,
    /// The node's publication masking, applied as uploads are written.
    pub(super) redaction: crate::redaction::Policy,
}

impl Target {
    pub(super) fn from_freshness(
        freshness: &super::Freshness,
        config: &crate::config::SharedConfig,
    ) -> Result<Self, AppError> {
        let group = freshness
            .group_views
            .as_ref()
            .filter(|_| freshness.group_id == config.group_id)
            .ok_or_else(|| AppError::shared("shared group views settings are unavailable"))?;
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
            redaction: config.publication_redaction,
        })
    }
}

/// One thread's ordered canonical dependency stream. No vectors, model identity,
/// checkpoint stamps or retrieval policy enter this digest.
pub(super) struct Fingerprint {
    digest: blake3::Hasher,
    chat_id: String,
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
    pub(super) fn new(chat: &ChatRecord) -> Result<Self, AppError> {
        let mut digest = blake3::Hasher::new();
        digest.update(b"mash-shared-thread-dependencies-v1\0");
        serde_json::to_writer(Digest(&mut digest), chat)
            .map_err(|_| AppError::internal("fingerprint shared thread"))?;
        digest.update(b"\n");
        Ok(Self {
            digest,
            chat_id: chat.id.clone(),
            ordinal: None,
        })
    }

    pub(super) fn push(&mut self, message: &MessageRecord) -> Result<(), AppError> {
        if message.chat_id != self.chat_id
            || message.ordinal < 0
            || self
                .ordinal
                .is_some_and(|previous| previous >= message.ordinal)
        {
            return Err(AppError::shared("invalid ordered thread dependencies"));
        }
        serde_json::to_writer(Digest(&mut self.digest), &MessageRef::from(message))
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
    chat_id: &'a str,
    ordinal: i64,
    #[serde(skip_serializing_if = "MessageContext::is_linear")]
    context: &'a MessageContext,
    role: &'a Role,
    content: &'a str,
    projection: &'a str,
    created_at: Option<i64>,
    #[serde(skip_serializing_if = "crate::tools::Activity::is_empty")]
    tools: &'a crate::tools::Activity,
}

impl<'a> From<&'a MessageRecord> for MessageRef<'a> {
    /// Canonical fields only; vectors and model identity are never silver.
    fn from(record: &'a MessageRecord) -> Self {
        Self {
            attachments: &record.attachments,
            id: &record.id,
            chat_id: &record.chat_id,
            ordinal: record.ordinal,
            context: &record.context,
            role: &record.role,
            content: &record.content,
            projection: &record.projection,
            created_at: record.created_at,
            tools: &record.tools,
        }
    }
}

#[cfg(test)]
mod tests;
