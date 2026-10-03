//! Persisted silver history and gold retrieval state share one atomic revision.
//! Transport/planning may join them, but canonical snapshot messages have no
//! embedding fields. Serialization borrows rows to avoid cloning whole nodes.
use super::{Checkpoint, ConversationRecord, Data, MessageRecord, Tombstone};
use crate::{AppError, message_context::MessageContext, role::Role};
use serde::{Deserialize, Serialize, ser::SerializeMap};
use std::collections::BTreeMap;

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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Silver {
    conversations: BTreeMap<String, ConversationRecord>,
    messages: BTreeMap<String, Message>,
    sources: BTreeMap<String, Checkpoint>,
    tombstones: BTreeMap<String, Tombstone>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    id: String,
    conversation_id: String,
    ordinal: i64,
    #[serde(default)]
    context: MessageContext,
    role: Role,
    content: String,
    projection: String,
    created_at: Option<i64>,
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
            sources: self.silver.sources,
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
struct SilverRef<'a> {
    conversations: &'a BTreeMap<String, ConversationRecord>,
    messages: MessagesRef<'a>,
    sources: &'a BTreeMap<String, Checkpoint>,
    tombstones: &'a BTreeMap<String, Tombstone>,
}
#[derive(Serialize)]
struct MessageRef<'a> {
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
struct MessagesRef<'a>(&'a BTreeMap<String, MessageRecord>);
impl Serialize for MessagesRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, m) in self.0 {
            map.serialize_entry(
                id,
                &MessageRef {
                    id: &m.id,
                    conversation_id: &m.conversation_id,
                    ordinal: m.ordinal,
                    context: m.context,
                    role: &m.role,
                    content: &m.content,
                    projection: &m.projection,
                    created_at: m.created_at,
                },
            )?;
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
    pub(in crate::shared) fn layers(&self) -> SnapshotRef<'_> {
        SnapshotRef {
            silver: SilverRef {
                conversations: &self.conversations,
                messages: MessagesRef(&self.messages),
                sources: &self.sources,
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
