//! Reconstruct shared views from shared silver alone, e.g. after losing Qdrant data.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::qdrant_index::{NodeViews, ViewChat};
use crate::shared::silver_store;
use crate::views::shared as views;
use std::path::Path;

/// Per-store interruption points; no global state or production fault switches.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::shared) enum ReplayFault {
    AfterInvalidation,
    AfterDeletion,
}

#[cfg(test)]
impl Store {
    pub(in crate::shared) fn with_replay_fault(mut self, fault: ReplayFault) -> Self {
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
pub(in crate::shared) struct Replayed {
    pub(in crate::shared) group_id: String,
    pub(in crate::shared) node_id: String,
    pub(in crate::shared) chats: u64,
    pub(in crate::shared) messages: u64,
    pub(in crate::shared) embeddings: u64,
}

impl Store {
    /// Re-embed every dense-eligible message and rewrite all of the node's views.
    #[allow(clippy::too_many_lines)]
    pub(in crate::shared) async fn replay(
        &self,
        identity: &Identity,
        models: &Path,
        embeddings: Option<&crate::semantic::remote::Configuration>,
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
        let recipe = self.views.build().clone();
        let mut pool = None;
        let mut build = views::Build::default();
        let mut replayed = Replayed {
            group_id: identity.group_id.clone(),
            node_id: identity.node_id.clone(),
            chats: 0,
            messages: 0,
            embeddings: 0,
        };
        let mut documents = crate::shared::qdrant_index::Documents::new(identity);
        for (id, fingerprint) in &fingerprints {
            let chat = id.clone();
            let (conversation, messages) = self
                .read(identity, move |connection| {
                    Ok((
                        silver_store::conversation(connection, &chat)?
                            .ok_or_else(|| AppError::internal("silver chat disappeared"))?,
                        silver_store::thread_messages(connection, &chat)?,
                    ))
                })
                .await?;
            let mut units = crate::views::units::Stream::default();
            let mut stored = 0;
            let mut expected = 0;
            // Match node-side inference batches, including hidden rows in each
            // boundary; quantized inference can depend on batch composition.
            for batch in messages.chunks(views::BATCH_ROWS) {
                let rows = batch.to_vec();
                let title = conversation.title.clone();
                let plan = recipe.clone();
                let (next, rows, prepared) = crate::blocking(move || {
                    let mut prepared = Vec::new();
                    for message in &rows {
                        prepared.push(units.prepare(
                            &plan,
                            crate::views::units::Input {
                                chat: &message.conversation_id,
                                ordinal: message.ordinal,
                                context: message.context,
                                role: &message.role,
                                projection: &message.projection,
                            },
                            title.as_deref(),
                        )?);
                    }
                    Ok((units, rows, prepared))
                })
                .await?;
                units = next;
                let texts = prepared
                    .iter()
                    .flatten()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                let mut vectors =
                    views::embed_texts(models, embeddings, &mut pool, &texts, &mut build)
                        .await?
                        .into_iter();
                if vectors.len() != texts.len() {
                    return Err(AppError::model("missing replayed shared embedding"));
                }
                let records = rows
                    .into_iter()
                    .zip(prepared)
                    .map(|(mut message, unit)| {
                        if unit.is_some() {
                            expected += 1;
                            message.vector = vectors.next();
                            message.generation = Some(identity.generation.clone());
                        }
                        message
                    })
                    .collect::<Vec<_>>();
                replayed.messages += records.len() as u64;
                for record in records {
                    stored += documents
                        .push(&self.views, identity, &conversation, record)
                        .await?;
                }
            }
            node.chats.insert(
                id.clone(),
                ViewChat {
                    fingerprint: fingerprint.clone(),
                    embeddings: stored,
                    complete: stored == expected,
                },
            );
            replayed.chats += 1;
            replayed.embeddings += stored;
        }
        documents.finish(&self.views).await?;
        self.certify(
            identity,
            node,
            &fingerprints,
            &crate::shared::activity::Progress::default(),
        )
        .await?;
        Ok(replayed)
    }
}
