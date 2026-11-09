//! Reconstruct shared views from shared silver alone, e.g. after losing Qdrant data.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::silver_store;
use crate::views::bulk::{BulkPolicy, BulkViews};
use crate::views::qdrant::{NodeViews, ProjectionEvidence};
use std::path::Path;
use std::time::Instant;

/// Per-store interruption points; no global state or production fault switches.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayFault {
    AfterInvalidation,
    AfterDeletion,
}

#[cfg(test)]
impl Store {
    pub(crate) fn with_replay_fault(mut self, fault: ReplayFault) -> Self {
        self.replay_fault = Some(fault);
        self
    }

    fn interrupt_replay(&self, point: ReplayFault) -> Result<(), AppError> {
        if self.replay_fault == Some(point) {
            return Err(AppError::internal("injected replay interruption"));
        }
        Ok(())
    }
}

/// Phase timings for one node replay. Durations are wall-clock milliseconds.
#[allow(
    clippy::struct_field_names,
    reason = "phase durations are all milliseconds; the suffix is the unit"
)]
#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct Timings {
    pub(crate) lock_ms: u64,
    pub(crate) begin_ms: u64,
    pub(crate) fingerprints_ms: u64,
    pub(crate) invalidate_ms: u64,
    pub(crate) delete_ms: u64,
    pub(crate) materialize_ms: u64,
    pub(crate) embedding_ms: u64,
    pub(crate) finish_ms: u64,
    pub(crate) certify_ms: u64,
    pub(crate) total_ms: u64,
}

fn millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Replay result for one node.
#[derive(Debug, serde::Serialize)]
pub(crate) struct Replayed {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) chats: u64,
    pub(crate) messages: u64,
    pub(crate) embeddings: u64,
    pub(crate) timings: Timings,
}

impl Store {
    /// Re-embed every dense-eligible message and rewrite all of the node's views.
    pub(crate) async fn replay(
        &self,
        identity: &Identity,
        models: &Path,
    ) -> Result<Replayed, AppError> {
        let total_started = Instant::now();
        let lock_started = Instant::now();
        let _node = self.lock(identity).await?;
        let _replay = self.replay_lock(&identity.group_id).await?;
        let lock_ms = millis(lock_started);
        let begin_started = Instant::now();
        let bulk = BulkViews::begin(&self.views, identity, BulkPolicy::shared_replay()).await?;
        let begin_ms = millis(begin_started);
        let mut replayed = bulk
            .run(
                || async {
                    let mut timings = Timings::default();
                    let phase = Instant::now();
                    let fingerprints = self
                        .read(identity, |connection| {
                            silver_store::fingerprints(connection)
                        })
                        .await?;
                    timings.fingerprints_ms = millis(phase);
                    // Metadata and documents live in separate collections. Persist
                    // stale certification before destructive writes so interruption
                    // fails closed.
                    let mut node = NodeViews::empty(&self.views, identity);
                    let phase = Instant::now();
                    bulk.invalidate(&node).await?;
                    timings.invalidate_ms = millis(phase);
                    #[cfg(test)]
                    self.interrupt_replay(ReplayFault::AfterInvalidation)?;
                    let phase = Instant::now();
                    bulk.delete_documents().await?;
                    timings.delete_ms = millis(phase);
                    #[cfg(test)]
                    self.interrupt_replay(ReplayFault::AfterDeletion)?;
                    let mut replayed = Replayed {
                        group_id: identity.group_id.clone(),
                        node_id: identity.node_id.clone(),
                        chats: 0,
                        messages: 0,
                        embeddings: 0,
                        timings,
                    };
                    let materialize_started = Instant::now();
                    let mut writer = bulk
                        .writer(models)
                        .with_execution(self.embeddings.clone())
                        .without_reuse();
                    for (id, fingerprint) in &fingerprints {
                        let chat = id.clone();
                        let (chat, messages) = self
                            .read(identity, move |connection| {
                                Ok((
                                    silver_store::chat(connection, &chat)?.ok_or_else(|| {
                                        AppError::internal("silver chat disappeared")
                                    })?,
                                    crate::storage::search_input::chat_messages(connection, &chat)?,
                                ))
                            })
                            .await?;
                        let built = writer.write_chat(&chat, &messages).await?;
                        node.projections.insert(
                            id.clone(),
                            ProjectionEvidence {
                                fingerprint: fingerprint.clone(),
                                embeddings: built.embeddings,
                                complete: built.complete,
                            },
                        );
                        replayed.chats += 1;
                        replayed.embeddings += built.embeddings;
                        replayed.messages += built.messages;
                    }
                    replayed.timings.materialize_ms = millis(materialize_started);
                    let phase = Instant::now();
                    let work = bulk.finish(writer).await?;
                    replayed.timings.finish_ms = millis(phase);
                    replayed.timings.embedding_ms = work.embedding_milliseconds;
                    // Every chat was rebuilt from the silver read above, so these
                    // views reflect the head the lock protects.
                    node.input = Some(self.head_revision(identity).await?);
                    let phase = Instant::now();
                    self.certify_bulk(
                        identity,
                        node,
                        &crate::shared::activity::Progress::default(),
                    )
                    .await?;
                    replayed.timings.certify_ms = millis(phase);
                    Ok(replayed)
                },
                |_| {},
            )
            .await?;
        replayed.timings.lock_ms = lock_ms;
        replayed.timings.begin_ms = begin_ms;
        replayed.timings.total_ms = millis(total_started);
        Ok(replayed)
    }
}
