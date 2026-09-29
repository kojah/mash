use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{Counts, SearchHit};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Start {
    pub instance: String,
    pub revision: Option<i64>,
    pub base_revision: Option<i64>,
    pub snapshot: bool,
    pub forgets: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Event {
    Start(Start),
    Batch {
        kind: String,
        records: Vec<Value>,
    },
    Source(Checkpoint),
    Seen {
        provider: String,
        hashes: Vec<String>,
    },
    Reconcile {
        provider: String,
        cutoff: Option<i64>,
    },
    Remove {
        id: String,
    },
    Finish,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Checkpoint {
    pub provider: String,
    pub path_hash: String,
    pub size_bytes: i64,
    pub modified_ns: i64,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub ready: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tombstone {
    pub provider: String,
    pub conversation_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
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
    pub query: String,
    #[serde(default = "default_generation")]
    pub generation: String,
    pub vector: Vec<f64>,
    pub provider: Option<String>,
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
