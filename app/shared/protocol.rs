use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::{Counts, SearchHit};

pub(super) const DIMENSIONS: usize = 384;

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
    pub vector: Vec<f64>,
    pub provider: Option<String>,
    pub days: Option<u32>,
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

pub(super) fn valid_vector(vector: &[f64]) -> bool {
    vector.len() == DIMENSIONS
        && vector
            .iter()
            .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))
        && vector.iter().any(|v| *v != 0.0)
}
