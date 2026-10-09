//! Commit silver first, then bring gold in line with it. A failed gold update
//! leaves gold marked stale; the next snapshot or a replay repairs it.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::protocol::{Batch, ConversationRecord, Event, Indexed};
use crate::shared::qdrant_index::{GoldChat, NodeGold};
use crate::shared::silver_store::{self, Applied};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufRead;

/// Documents written per Qdrant request.
const DOCUMENT_BATCH: usize = 64;

/// A chat being written from an upload or from silver.
#[derive(Default)]
struct Written {
    ids: Vec<String>,
    embeddings: u64,
    expected: u64,
}

impl Store {
    #[tracing::instrument(name = "backend_index", skip_all, fields(backend = "sqlite+qdrant"))]
    pub(in crate::shared) async fn index(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        let _node = self.lock(identity).await?;
        self.gold.gold_configuration(identity).await?;
        let previous = self.head(identity).await?;
        let gold = self.compatible_gold(identity).await?;
        let silver_revision = previous.as_ref().and_then(|h| h.silver_revision.clone());
        let current = gold
            .as_ref()
            .is_some_and(|g| g.current(&self.gold, identity, silver_revision.as_ref()));
        let silver = self.silver.clone();
        let scope = identity.clone();
        let build = self.gold.build().clone();
        let (applied, file) = crate::blocking(move || {
            let applied = silver.write(&scope, |tx| {
                silver_store::apply::apply(tx, &scope, &file, &build)
            })?;
            Ok((applied, file))
        })
        .await?;
        if !(applied.duplicate && current) {
            self.sync_upload(identity, &file, &applied, gold).await?;
        }
        Ok(Indexed {
            counts: self.status(identity).await?,
            revision: applied.revision,
            removed_messages: applied.removed,
            purged_conversations: applied.purged,
        })
    }

    /// Write gold for the chats an upload supplied, then certify the revision.
    async fn sync_upload(
        &self,
        identity: &Identity,
        file: &tempfile::NamedTempFile,
        applied: &Applied,
        previous: Option<NodeGold>,
    ) -> Result<(), AppError> {
        // Without prior gold every document is deleted, so none can be stale.
        let fresh = previous.is_none();
        let mut gold = if let Some(gold) = previous {
            gold
        } else {
            self.gold.delete_node_documents(identity).await?;
            NodeGold::empty(&self.gold, identity)
        };
        gold.silver = None;
        self.gold.put_node_gold(identity, &gold).await?;
        for id in &applied.deleted {
            self.gold.retain_documents(identity, id, Vec::new()).await?;
            gold.chats.remove(id);
        }
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        // A duplicate snapshot resupplies every chat with its vectors.
        let supplied: BTreeSet<String> = if applied.duplicate {
            fingerprints.keys().cloned().collect()
        } else {
            applied.supplied.clone()
        };
        let written = self.write_upload(identity, file, &supplied).await?;
        for id in &supplied {
            let chat = written.get(id);
            if !fresh {
                let current = chat.map(|t| t.ids.clone()).unwrap_or_default();
                self.gold.retain_documents(identity, id, current).await?;
            }
            let fingerprint = fingerprints
                .get(id)
                .cloned()
                .ok_or_else(|| AppError::internal("supplied chat has no silver"))?;
            gold.chats.insert(
                id.clone(),
                GoldChat {
                    fingerprint,
                    embeddings: chat.map_or(0, |t| t.embeddings),
                    complete: chat.is_none_or(|t| t.embeddings == t.expected),
                },
            );
        }
        self.certify(identity, gold, &fingerprints).await
    }

    /// Stream the upload's messages for the selected chats into documents.
    async fn write_upload(
        &self,
        identity: &Identity,
        file: &tempfile::NamedTempFile,
        selected: &BTreeSet<String>,
    ) -> Result<BTreeMap<String, Written>, AppError> {
        let mut lines = std::io::BufReader::new(file.reopen().map_err(AppError::io)?).lines();
        let mut written = BTreeMap::<String, Written>::new();
        let mut conversations = HashMap::<String, ConversationRecord>::new();
        let build = self.gold.build().clone();
        let mut documents = crate::shared::qdrant_index::Documents::new(identity);
        loop {
            let (next, batch) = crate::blocking(move || {
                let mut batch = Vec::new();
                for line in lines.by_ref() {
                    let line = line.map_err(AppError::io)?;
                    if let Event::Batch(Batch::Messages { records }) =
                        serde_json::from_str(&line)
                            .map_err(|_| AppError::usage("invalid index event"))?
                    {
                        batch.extend(records);
                        if batch.len() >= DOCUMENT_BATCH {
                            break;
                        }
                    }
                }
                Ok((lines, batch))
            })
            .await?;
            lines = next;
            if batch.is_empty() {
                break;
            }
            for message in batch {
                let id = message.conversation_id.clone();
                if !selected.contains(&id) {
                    continue;
                }
                if !conversations.contains_key(&id) {
                    let chat = id.clone();
                    let record = self
                        .read(identity, move |connection| {
                            silver_store::conversation(connection, &chat)
                        })
                        .await?
                        .ok_or_else(|| AppError::internal("supplied chat has no silver"))?;
                    conversations.insert(id.clone(), record);
                }
                let entry = written.entry(id.clone()).or_default();
                entry.ids.push(message.id.clone());
                entry.expected +=
                    u64::from(build.includes_dense(&message.role, &message.projection));
                entry.embeddings += documents
                    .push(&self.gold, identity, &conversations[&id], message)
                    .await?;
            }
        }
        documents.finish(&self.gold).await?;
        Ok(written)
    }

    /// Record gold as current only when every silver chat has complete documents.
    pub(in crate::shared) async fn certify(
        &self,
        identity: &Identity,
        mut gold: NodeGold,
        fingerprints: &BTreeMap<String, String>,
    ) -> Result<(), AppError> {
        self.sync_connectors(identity, &mut gold).await?;
        gold.chats.retain(|id, _| fingerprints.contains_key(id));
        let head = self.head(identity).await?;
        let synced = gold.chats.len() == fingerprints.len()
            && fingerprints.iter().all(|(id, fingerprint)| {
                gold.chats
                    .get(id)
                    .is_some_and(|t| &t.fingerprint == fingerprint)
            });
        gold.certified = synced && gold.chats.values().all(|chat| chat.complete);
        gold.silver = synced
            .then(|| head.and_then(|h| h.silver_revision))
            .flatten();
        self.gold.put_node_gold(identity, &gold).await?;
        self.gold.settle(identity).await
    }
}
