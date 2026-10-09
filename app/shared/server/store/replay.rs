//! Reconstruct shared gold from shared silver alone, e.g. after losing Qdrant data.
use super::{Identity, Store};
use crate::AppError;
use crate::gold::shared as gold;
use crate::shared::qdrant_index::{GoldChat, NodeGold};
use crate::shared::silver_store;
use std::path::Path;

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
    /// Re-embed every dense-eligible message and rewrite all of the node's gold.
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
        self.gold.delete_node_documents(identity).await?;
        let mut node = NodeGold::empty(&self.gold, identity);
        self.gold.put_node_gold(identity, &node).await?;
        let recipe = self.gold.build().clone();
        let mut pool = None;
        let mut build = gold::Build::default();
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
            let mut units = crate::gold::units::Stream::default();
            let mut stored = 0;
            let mut expected = 0;
            // Match node-side inference batches, including hidden rows in each
            // boundary; quantized inference can depend on batch composition.
            for batch in messages.chunks(gold::BATCH_ROWS) {
                let rows = batch.to_vec();
                let title = conversation.title.clone();
                let plan = recipe.clone();
                let (next, rows, prepared) = crate::blocking(move || {
                    let mut prepared = Vec::new();
                    for message in &rows {
                        prepared.push(units.prepare(
                            &plan,
                            crate::gold::units::Input {
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
                    gold::embed_texts(models, embeddings, &mut pool, &texts, &mut build)
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
                        .push(&self.gold, identity, &conversation, record)
                        .await?;
                }
            }
            node.chats.insert(
                id.clone(),
                GoldChat {
                    fingerprint: fingerprint.clone(),
                    embeddings: stored,
                    complete: stored == expected,
                },
            );
            replayed.chats += 1;
            replayed.embeddings += stored;
        }
        documents.finish(&self.gold).await?;
        self.certify(identity, node, &fingerprints).await?;
        Ok(replayed)
    }
}
