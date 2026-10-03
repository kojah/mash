use crate::provider::Provider;
use serde::{Deserialize, Serialize};

use crate::storage::{Counts, SearchHit};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    pub instance: String,
    pub revision: Option<i64>,
    pub base_revision: Option<i64>,
    pub snapshot: bool,
    pub forgets: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    Start(Start),
    Batch(Batch),
    Source(Checkpoint),
    Seen {
        provider: Provider,
        hashes: Vec<String>,
    },
    Reconcile {
        provider: Provider,
        cutoff: Option<i64>,
    },
    Remove {
        id: String,
    },
    Finish,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Batch {
    Tombstones { records: Vec<Tombstone> },
    Conversations { records: Vec<ConversationRecord> },
    Messages { records: Vec<MessageRecord> },
}

#[derive(Clone, Copy)]
pub(crate) enum BatchKind {
    Tombstones,
    Conversations,
    Messages,
}

impl Batch {
    pub(crate) fn take(&mut self) -> Self {
        match self {
            Self::Tombstones { records } => Self::Tombstones {
                records: std::mem::take(records),
            },
            Self::Conversations { records } => Self::Conversations {
                records: std::mem::take(records),
            },
            Self::Messages { records } => Self::Messages {
                records: std::mem::take(records),
            },
        }
    }
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Tombstones { records } => records.len(),
            Self::Conversations { records } => records.len(),
            Self::Messages { records } => records.len(),
        }
    }

    pub(super) fn records_json(&self) -> Result<String, serde_json::Error> {
        match self {
            Self::Tombstones { records } => serde_json::to_string(records),
            Self::Conversations { records } => serde_json::to_string(records),
            Self::Messages { records } => serde_json::to_string(records),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationRecord {
    pub id: String,
    pub provider: Provider,
    pub title: Option<String>,
    pub project: Option<String>,
    pub repository: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub ordinal: i64,
    #[serde(
        default,
        skip_serializing_if = "crate::message_context::MessageContext::is_linear"
    )]
    pub context: crate::message_context::MessageContext,
    pub role: crate::role::Role,
    pub content: String,
    pub projection: String,
    pub created_at: Option<i64>,
    pub generation: Option<String>,
    pub vector: Option<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    pub provider: Provider,
    pub path_hash: String,
    pub size_bytes: i64,
    pub modified_ns: i64,
    #[serde(default)]
    pub fingerprint: String,
    /// Missing versions are unverified; old checkpoints must be replayed.
    #[serde(default)]
    pub interpretation_version: u32,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tombstone {
    pub provider: Provider,
    pub conversation_id: String,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct NodeState {
    pub instance: Option<String>,
    pub revision: i64,
    pub sources: Vec<Checkpoint>,
    pub tombstones: Vec<Tombstone>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Indexed {
    pub counts: Counts,
    pub revision: i64,
    pub removed_messages: u64,
    pub purged_conversations: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    #[serde(default)]
    pub unit: crate::search_results::SearchUnit,
    pub query: String,
    #[serde(default = "default_generation")]
    pub generation: String,
    pub vector: Vec<f64>,
    pub provider: Option<Provider>,
    pub days: Option<u32>,
    #[serde(default)]
    pub exclude_self: bool,
    pub filter: Option<crate::search_filter::Filter>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Candidates {
    pub lexical: Vec<SearchHit>,
    pub semantic: Vec<SearchHit>,
    pub documents: Vec<Document>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Document {
    pub node_id: String,
    pub id: String,
    pub projection: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct View {
    pub node_id: String,
    pub id: String,
    pub context: u32,
    #[serde(
        default,
        skip_serializing_if = "crate::storage::MessageContent::is_canonical"
    )]
    pub content: crate::storage::MessageContent,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Forget {
    pub instance: String,
    pub revision: Option<i64>,
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Forgotten {
    pub forgotten: bool,
}

pub(super) fn default_generation() -> String {
    crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned()
}

pub(super) fn valid_vector(vector: &[f64], dimensions: usize) -> bool {
    vector.len() == dimensions
        && vector.iter().all(|v| v.is_finite() && v.abs() <= 127.0)
        && vector
            .iter()
            .any(|v| v.abs() >= f64::from(f32::MIN_POSITIVE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_validate_record_schema_at_the_protocol_boundary() {
        let line = r#"{"type":"batch","kind":"tombstones","records":[{"provider":"codex","conversation_id":"gone"}]}"#;
        let event: Event = serde_json::from_str(line).unwrap();
        let encoded = serde_json::to_string(&event).unwrap();
        let Event::Batch(Batch::Tombstones { records }) = serde_json::from_str(&encoded).unwrap()
        else {
            panic!("typed tombstone batch")
        };
        assert_eq!(records[0].conversation_id, "gone");
        for invalid in [
            r#"{"type":"batch","kind":"messages","records":[{"provider":"codex","conversation_id":"gone"}]}"#,
            r#"{"type":"batch","kind":"unknown","records":[]}"#,
            r#"{"type":"batch","kind":"tombstones","records":[{"provider":"unknown","conversation_id":"gone"}]}"#,
            r#"{"type":"batch","kind":"tombstones","records":[{"provider":"codex","conversation_id":3}]}"#,
            r#"{"type":"batch","kind":"tombstones","records":[],"unexpected":true}"#,
        ] {
            assert!(serde_json::from_str::<Event>(invalid).is_err(), "{invalid}");
        }
    }
}
