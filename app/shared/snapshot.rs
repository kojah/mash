//! Backend-independent canonical node snapshot validation and planning.
use super::backend::{conflict, valid_id};
use super::protocol::{
    self, Batch, Checkpoint, ConversationRecord, Event, MessageRecord, NodeState, Start, Tombstone,
};
use super::server::Identity;
use crate::provider::Provider;
use crate::storage::Counts;
use crate::{AppError, semantic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub(super) mod encoding;
pub(super) mod payload;
mod plan;
pub(super) use encoding::Encoded;
pub(super) use plan::{forget, plan};
fn identity_key(provider: Provider, id: &str) -> String {
    format!("{provider}\0{id}")
}
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Head {
    pub(super) instance: String,
    pub(super) generation: String,
    pub(super) revision: i64,
    pub(super) version: String,
    pub(super) counts: Counts,
    pub(super) last_synced_at: Option<String>,
    pub(super) last_updated_at: Option<String>,
    #[serde(default)]
    pub(super) metadata_ready: bool,
    #[serde(default)]
    pub(super) catalog_ready: bool,
}
#[derive(Default, Deserialize, Serialize)]
pub(super) struct Data {
    pub(super) conversations: BTreeMap<String, ConversationRecord>,
    pub(super) messages: BTreeMap<String, MessageRecord>,
    pub(super) sources: BTreeMap<String, Checkpoint>,
    pub(super) tombstones: BTreeMap<String, Tombstone>,
}
pub(super) struct Loaded {
    pub(super) digest: Option<String>,
    pub(super) head: Option<Head>,
    pub(super) expected: String,
    pub(super) data: Data,
}
/// Keep decoding and mutation planning on one worker to reuse freed allocations.
pub(super) struct PendingSnapshot {
    pub(super) payload: Option<payload::Received>,
    pub(super) head: Option<Head>,
    pub(super) expected: String,
}
impl PendingSnapshot {
    pub(in crate::shared) fn encoded_bytes(&self) -> usize {
        self.payload
            .as_ref()
            .map_or(0, payload::Received::encoded_bytes)
    }
    pub(in crate::shared) fn decode(self) -> Result<Loaded, AppError> {
        let digest = self.payload.as_ref().map(|p| p.digest.clone());
        let data = self
            .payload
            .map_or_else(|| Ok(Data::default()), payload::Received::decode)?;
        Ok(Loaded {
            digest,
            head: self.head,
            expected: self.expected,
            data,
        })
    }
}
pub(super) struct Plan {
    pub(super) loaded: Loaded,
    pub(super) revision: i64,
    pub(super) changed: bool,
    pub(super) removed: u64,
    pub(super) purged: u64,
}
