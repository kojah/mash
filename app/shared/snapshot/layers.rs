//! Persisted silver history and gold retrieval state share one atomic revision.
//! Transport/planning may join them, but canonical snapshot messages have no
//! embedding fields. Serialization borrows rows to avoid cloning whole nodes.
use super::{Checkpoint, ConversationRecord, Data, MessageRecord, Tombstone};
use crate::{AppError, message_context::MessageContext, role::Role};
use serde::{Deserialize, Serialize, ser::SerializeMap};
use std::collections::BTreeMap;
mod sources;
use sources::{Facts, SourcesRef};

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::shared) enum Schema {
    #[default]
    Mixed,
    Layered,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::shared) struct Snapshot {
    silver: Silver,
    gold: Gold,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::shared) struct Silver {
    pub(in crate::shared) conversations: BTreeMap<String, ConversationRecord>,
    pub(in crate::shared) messages: BTreeMap<String, Message>,
    pub(in crate::shared) sources: BTreeMap<String, Facts>,
    pub(in crate::shared) tombstones: BTreeMap<String, Tombstone>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::shared) struct Message {
    #[serde(default)]
    pub(in crate::shared) attachments: Vec<crate::attachments::Metadata>,
    pub(in crate::shared) id: String,
    pub(in crate::shared) conversation_id: String,
    pub(in crate::shared) ordinal: i64,
    #[serde(default)]
    pub(in crate::shared) context: MessageContext,
    pub(in crate::shared) role: Role,
    pub(in crate::shared) content: String,
    pub(in crate::shared) projection: String,
    pub(in crate::shared) created_at: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gold {
    embeddings: BTreeMap<String, Embedding>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Embedding {
    generation: Option<String>,
    vector: Option<Vec<f64>>,
}
impl Snapshot {
    pub(in crate::shared) fn into_data(mut self) -> Result<Data, AppError> {
        let mut messages = BTreeMap::new();
        for (id, message) in self.silver.messages {
            if id != message.id {
                return Err(AppError::shared("invalid silver message identity"));
            }
            let embedding = self.gold.embeddings.remove(&id);
            let (generation, vector) = embedding.map_or((None, None), |e| (e.generation, e.vector));
            if message.projection.is_empty() && (generation.is_some() || vector.is_some()) {
                return Err(AppError::shared(
                    "context-only silver message has gold embedding",
                ));
            }
            messages.insert(
                id,
                MessageRecord {
                    attachments: message.attachments,
                    id: message.id,
                    conversation_id: message.conversation_id,
                    ordinal: message.ordinal,
                    context: message.context,
                    role: message.role,
                    content: message.content,
                    projection: message.projection,
                    created_at: message.created_at,
                    generation,
                    vector,
                },
            );
        }
        if !self.gold.embeddings.is_empty() {
            return Err(AppError::shared("gold embedding has no silver message"));
        }
        Ok(Data {
            conversations: self.silver.conversations,
            messages,
            sources: self
                .silver
                .sources
                .into_iter()
                .map(|(id, source)| (id, source.checkpoint()))
                .collect(),
            tombstones: self.silver.tombstones,
        })
    }
}

#[derive(Serialize)]
pub(in crate::shared) struct SnapshotRef<'a> {
    silver: SilverRef<'a>,
    gold: GoldRef<'a>,
}
#[derive(Serialize)]
pub(in crate::shared) struct SilverRef<'a> {
    conversations: &'a BTreeMap<String, ConversationRecord>,
    messages: MessagesRef<'a>,
    sources: SourcesRef<'a>,
    tombstones: &'a BTreeMap<String, Tombstone>,
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
struct MessagesRef<'a>(&'a BTreeMap<String, MessageRecord>);
impl Serialize for MessagesRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, m) in self.0 {
            map.serialize_entry(id, &m.silver_ref())?;
        }
        map.end()
    }
}
#[derive(Serialize)]
struct GoldRef<'a> {
    embeddings: EmbeddingsRef<'a>,
}
#[derive(Serialize)]
struct EmbeddingRef<'a> {
    generation: Option<&'a str>,
    vector: Option<&'a [f64]>,
}
struct EmbeddingsRef<'a>(&'a BTreeMap<String, MessageRecord>);
impl Serialize for EmbeddingsRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (id, m) in self.0 {
            if m.generation.is_some() || m.vector.is_some() {
                map.serialize_entry(
                    id,
                    &EmbeddingRef {
                        generation: m.generation.as_deref(),
                        vector: m.vector.as_deref(),
                    },
                )?;
            }
        }
        map.end()
    }
}
impl Data {
    /// Hash canonical fields in deterministic order without allocating body copies.
    /// Checkpoints and model vectors are build dependencies, not canonical edits.
    pub(in crate::shared) fn canonical_fingerprint(&self) -> Result<String, AppError> {
        #[derive(Serialize)]
        struct Canonical<'a> {
            conversations: &'a BTreeMap<String, ConversationRecord>,
            messages: MessagesRef<'a>,
            tombstones: &'a BTreeMap<String, Tombstone>,
        }
        struct Digest(blake3::Hasher);
        impl std::io::Write for Digest {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.update(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut digest = Digest(blake3::Hasher::new());
        digest.0.update(b"mash-shared-silver-v1\0");
        let mut output = std::io::BufWriter::with_capacity(64 * 1024, &mut digest);
        serde_json::to_writer(
            &mut output,
            &Canonical {
                conversations: &self.conversations,
                messages: MessagesRef(&self.messages),
                tombstones: &self.tombstones,
            },
        )
        .map_err(|_| AppError::internal("fingerprint shared silver"))?;
        std::io::Write::flush(&mut output)
            .map_err(|_| AppError::internal("flush shared silver fingerprint"))?;
        drop(output);
        Ok(digest.0.finalize().to_hex().to_string())
    }

    pub(in crate::shared) fn silver(&self) -> SilverRef<'_> {
        SilverRef {
            conversations: &self.conversations,
            messages: MessagesRef(&self.messages),
            sources: SourcesRef(&self.sources),
            tombstones: &self.tombstones,
        }
    }

    pub(in crate::shared) fn layers(&self) -> SnapshotRef<'_> {
        SnapshotRef {
            silver: SilverRef {
                conversations: &self.conversations,
                messages: MessagesRef(&self.messages),
                sources: SourcesRef(&self.sources),
                tombstones: &self.tombstones,
            },
            gold: GoldRef {
                embeddings: EmbeddingsRef(&self.messages),
            },
        }
    }
}

#[cfg(test)]
mod tests;
