//! Reconstruct shared views from shared silver alone, e.g. after losing Qdrant data.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::silver_store;
use crate::views::materialize::Writer;
use crate::views::qdrant::{NodeViews, ProjectionEvidence};
use std::path::Path;

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

/// Replay result for one node.
#[derive(Debug, serde::Serialize)]
pub(crate) struct Replayed {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) chats: u64,
    pub(crate) messages: u64,
    pub(crate) embeddings: u64,
}

impl Store {
    /// Re-embed every dense-eligible message and rewrite all of the node's views.
    pub(crate) async fn replay(
        &self,
        identity: &Identity,
        models: &Path,
    ) -> Result<Replayed, AppError> {
        let _node = self.lock(identity).await?;
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        // Metadata and documents live in separate collections. Persist stale
        // certification before destructive writes so interruption fails closed.
        let mut node = NodeViews::empty(&self.views, identity);
        self.views.put_node_views(identity, &node).await?;
        #[cfg(test)]
        self.interrupt_replay(ReplayFault::AfterInvalidation)?;
        self.views.delete_node_documents(identity).await?;
        #[cfg(test)]
        self.interrupt_replay(ReplayFault::AfterDeletion)?;
        let mut replayed = Replayed {
            group_id: identity.group_id.clone(),
            node_id: identity.node_id.clone(),
            chats: 0,
            messages: 0,
            embeddings: 0,
        };
        let mut writer = Writer::new(&self.views, identity, Some(models));
        for (id, fingerprint) in &fingerprints {
            let chat = id.clone();
            let (chat, messages) = self
                .read(identity, move |connection| {
                    Ok((
                        silver_store::chat(connection, &chat)?
                            .ok_or_else(|| AppError::internal("silver chat disappeared"))?,
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
        writer.finish().await?;
        // Every chat was rebuilt from the silver read above, so these views
        // reflect the head the lock protects.
        node.input = Some(self.head_revision(identity).await?);
        self.certify(
            identity,
            node,
            &crate::shared::activity::Progress::default(),
        )
        .await?;
        Ok(replayed)
    }
}
