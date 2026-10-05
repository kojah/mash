//! Backend-independent publication planning and atomic silver/gold revisions.
use super::protocol::{
    self, Batch, Checkpoint, ConversationRecord, Event, MessageRecord, NodeState, Start, Tombstone,
};
use super::server::Identity;
use super::validation::{conflict, valid_id};
use crate::provider::Provider;
use crate::storage::Counts;
use crate::{AppError, semantic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub(super) mod encoding;
pub(super) mod layers;
pub(super) mod payload;
mod plan;
pub(super) use encoding::Encoded;
pub(super) use plan::{forget, plan};
fn identity_key(provider: Provider, id: &str) -> String {
    format!("{provider}\0{id}")
}
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Head {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) lexical_manifest: Option<super::lexical_manifest::Manifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) gold_manifest: Option<super::gold_manifest::Manifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) silver_coverage: Vec<super::silver_coverage::Coverage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) silver_revision: Option<super::silver_revision::State>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) silver_scans: Vec<protocol::ScanReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_silver: Option<crate::silver::certificate::Certificate>,
    #[serde(default)]
    pub(super) snapshot_schema: layers::Schema,
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
/// Joined transport/planning rows; production persistence uses `layers()`.
/// Only migration fixtures can serialize the historical mixed representation.
#[derive(Default, Deserialize)]
#[cfg_attr(test, derive(Serialize))]
pub(super) struct Data {
    pub(super) conversations: BTreeMap<String, ConversationRecord>,
    pub(super) messages: BTreeMap<String, MessageRecord>,
    pub(super) sources: BTreeMap<String, Checkpoint>,
    pub(super) tombstones: BTreeMap<String, Tombstone>,
}
pub(super) struct Loaded {
    pub(super) head: Option<Head>,
    pub(super) expected: String,
    pub(super) data: Data,
}
/// Keep decoding and mutation planning on one worker to reuse freed allocations.
pub(super) struct PendingSnapshot {
    pub(super) payload: Option<payload::Received>,
    pub(super) threads: Option<Vec<(String, payload::Received)>>,
    pub(super) head: Option<Head>,
    pub(super) expected: String,
}
impl PendingSnapshot {
    pub(in crate::shared) fn encoded_bytes(&self) -> usize {
        self.threads
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|(_, part)| part.encoded_bytes())
            .sum::<usize>()
            + self
                .payload
                .as_ref()
                .map_or(0, payload::Received::encoded_bytes)
    }
    #[tracing::instrument(name = "shared_join_snapshot", skip_all)]
    pub(in crate::shared) fn decode(self) -> Result<Loaded, AppError> {
        let mut data = match self.payload {
            None => Data::default(),
            Some(payload) => match self
                .head
                .as_ref()
                .map_or_else(layers::Schema::default, |h| h.snapshot_schema)
            {
                layers::Schema::Mixed => payload.decode::<Data>()?,
                layers::Schema::Layered => payload.decode::<layers::Snapshot>()?.into_data()?,
            },
        };
        let partitioned = self.threads.is_some();
        if partitioned && (!data.conversations.is_empty() || !data.messages.is_empty()) {
            return Err(AppError::shared("thread metadata contains history records"));
        }
        for (id, payload) in self.threads.unwrap_or_default() {
            let part = payload.decode::<layers::Snapshot>()?.into_data()?;
            if part.conversations.len() != 1
                || !part.conversations.contains_key(&id)
                || !part.sources.is_empty()
                || !part.tombstones.is_empty()
                || part
                    .messages
                    .values()
                    .any(|message| message.conversation_id != id)
                || data
                    .conversations
                    .insert(
                        id,
                        part.conversations
                            .into_values()
                            .next()
                            .ok_or_else(|| AppError::shared("missing thread snapshot"))?,
                    )
                    .is_some()
            {
                return Err(AppError::shared("invalid retained thread snapshot"));
            }
            for (key, message) in part.messages {
                if data.messages.insert(key, message).is_some() {
                    return Err(AppError::shared("duplicate retained message"));
                }
            }
        }
        if partitioned
            && self.head.as_ref().is_none_or(|head| {
                head.counts.conversations != data.conversations.len() as u64
                    || head.counts.messages != data.messages.len() as u64
            })
        {
            return Err(AppError::shared(
                "thread snapshot counts disagree with publication",
            ));
        }
        Ok(Loaded {
            head: self.head,
            expected: self.expected,
            data,
        })
    }
}
pub(super) struct Plan {
    // Recipe validated for source-supplied and retained vectors. Gold-only or
    // forget publication instead carries forward its explicit prior manifest.
    pub(super) source_build: Option<crate::gold::config::Build>,
    pub(super) loaded: Loaded,
    pub(super) revision: i64,
    pub(super) changed: bool,
    pub(super) removed: u64,
    pub(super) purged: u64,
}

impl Plan {
    /// Compute on the publication worker, before any stage becomes visible.
    pub(in crate::shared) fn silver_revision(
        &self,
    ) -> Result<super::silver_revision::State, AppError> {
        super::silver_revision::State::next(
            self.loaded
                .head
                .as_ref()
                .and_then(|head| head.silver_revision.as_ref()),
            self.loaded.data.canonical_fingerprint()?,
        )
    }
}
