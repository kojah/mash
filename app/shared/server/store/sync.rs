//! Commit silver first, then bring gold in line with it. A failed gold update
//! leaves gold marked stale; the next snapshot or a replay repairs it.
use super::{Identity, Store, certified};
use crate::AppError;
use crate::shared::protocol::{
    self, Batch, ConversationRecord, Event, Forget, Forgotten, Indexed, MessageRecord,
};
use crate::shared::qdrant_index::{GoldThread, NodeGold};
use crate::shared::silver_store::{self, Applied, Head, apply::GoldView};
use crate::shared::validation::conflict;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufRead;

/// Documents written per Qdrant request.
const DOCUMENT_BATCH: usize = 64;

/// A thread being written from an upload or from silver.
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
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        let view = GoldView {
            build: current.then(|| self.gold.build().clone()),
            reusable: Self::reusable(gold.as_ref(), current, &fingerprints),
        };
        let silver = self.silver.clone();
        let scope = identity.clone();
        let build = self.gold.build().clone();
        let (applied, file) = crate::blocking(move || {
            let applied = silver.write(&scope, |tx| {
                silver_store::apply::apply(tx, &scope, &file, &build, &view)
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

    /// Write gold for the threads an upload supplied, then certify the revision.
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
            gold.threads.remove(id);
        }
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        // A duplicate snapshot resupplies every thread with its vectors.
        let supplied: BTreeSet<String> = if applied.duplicate {
            fingerprints.keys().cloned().collect()
        } else {
            applied
                .supplied
                .difference(&applied.reused)
                .cloned()
                .collect()
        };
        let written = self.write_upload(identity, file, &supplied).await?;
        for id in &supplied {
            let thread = written.get(id);
            if !fresh {
                let current = thread.map(|t| t.ids.clone()).unwrap_or_default();
                self.gold.retain_documents(identity, id, current).await?;
            }
            let fingerprint = fingerprints
                .get(id)
                .cloned()
                .ok_or_else(|| AppError::internal("supplied thread has no silver"))?;
            gold.threads.insert(
                id.clone(),
                GoldThread {
                    fingerprint,
                    embeddings: thread.map_or(0, |t| t.embeddings),
                    complete: thread.is_none_or(|t| t.embeddings == t.expected),
                },
            );
        }
        self.certify(identity, gold, &fingerprints).await
    }

    /// Stream the upload's messages for the selected threads into documents.
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
                    let thread = id.clone();
                    let record = self
                        .read(identity, move |connection| {
                            silver_store::conversation(connection, &thread)
                        })
                        .await?
                        .ok_or_else(|| AppError::internal("supplied thread has no silver"))?;
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

    /// Record gold as current only when every silver thread has complete documents.
    pub(in crate::shared) async fn certify(
        &self,
        identity: &Identity,
        mut gold: NodeGold,
        fingerprints: &BTreeMap<String, String>,
    ) -> Result<(), AppError> {
        gold.threads.retain(|id, _| fingerprints.contains_key(id));
        let head = self.head(identity).await?;
        let synced = gold.threads.len() == fingerprints.len()
            && fingerprints.iter().all(|(id, fingerprint)| {
                gold.threads
                    .get(id)
                    .is_some_and(|t| &t.fingerprint == fingerprint)
            });
        gold.certified = synced && gold.threads.values().all(|thread| thread.complete);
        gold.silver = synced
            .then(|| head.and_then(|h| h.silver_revision))
            .flatten();
        self.gold.put_node_gold(identity, &gold).await?;
        self.gold.settle(identity).await
    }

    /// Replace this node's dense vectors for its current, certified silver.
    #[allow(clippy::too_many_lines)]
    pub(in crate::shared) async fn rebuild_gold(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        use crate::shared::gold_rebuild::Event as GoldEvent;
        let _node = self.lock(identity).await?;
        self.gold.gold_configuration(identity).await?;
        let scope = identity.clone();
        let build = self.gold.build().clone();
        let (head, vectors) = self
            .read(identity, move |connection| {
                let mut lines =
                    std::io::BufReader::new(file.reopen().map_err(AppError::io)?).lines();
                let first = lines
                    .next()
                    .ok_or_else(|| AppError::usage("empty gold stream"))?
                    .map_err(AppError::io)?;
                let GoldEvent::Start {
                    publication_revision,
                    silver,
                    generation,
                    build: requested,
                } = serde_json::from_str(&first)
                    .map_err(|_| AppError::usage("invalid gold start"))?
                else {
                    return Err(AppError::usage("gold stream must start with metadata"));
                };
                silver
                    .validate()
                    .map_err(|_| AppError::usage("invalid gold silver identity"))?;
                let head = silver_store::head(connection)?
                    .ok_or_else(|| conflict("shared silver has not been indexed"))?;
                if head.revision != publication_revision
                    || head.silver_revision.as_ref() != Some(&silver)
                {
                    return Err(conflict(
                        "shared silver or publication changed; restart gold rebuild",
                    ));
                }
                if generation != scope.generation {
                    return Err(AppError::usage(
                        "gold model does not match shared embedding space",
                    ));
                }
                if requested != build {
                    return Err(conflict("shared gold recipe changed; restart gold rebuild"));
                }
                if !certified(connection, &head)? {
                    return Err(conflict(
                        "gold rebuild requires complete, current shared silver",
                    ));
                }
                let mut vectors = HashMap::new();
                let mut finished = false;
                for line in lines {
                    if finished {
                        return Err(AppError::usage("data after gold finish"));
                    }
                    match serde_json::from_str(&line.map_err(AppError::io)?)
                        .map_err(|_| AppError::usage("invalid gold event"))?
                    {
                        GoldEvent::Embeddings { records } => {
                            if records.len() > 128 {
                                return Err(AppError::usage("gold batch exceeds 128 records"));
                            }
                            for record in records {
                                let message =
                                    silver_store::message(connection, &record.message_id)?
                                        .ok_or_else(|| {
                                            AppError::usage("gold embedding has no silver message")
                                        })?;
                                if !build.includes_dense(&message.role, &message.projection)
                                    || !protocol::valid_vector(&record.vector, scope.dimensions)
                                    || vectors.insert(record.message_id, record.vector).is_some()
                                {
                                    return Err(AppError::usage(
                                        "invalid or duplicate gold embedding",
                                    ));
                                }
                            }
                        }
                        GoldEvent::Finish => finished = true,
                        GoldEvent::Start { .. } => {
                            return Err(AppError::usage("duplicate gold start"));
                        }
                    }
                }
                let expected = silver_store::dense_messages(connection, &build)?;
                if !finished || vectors.len() as u64 != expected {
                    return Err(AppError::usage("incomplete gold replacement"));
                }
                Ok((head, vectors))
            })
            .await?;
        self.write_silver(identity, |message| vectors.get(&message.id).cloned())
            .await?;
        // Gold-only changes still advance the publication token that uploads pin.
        let scope = identity.clone();
        let silver = self.silver.clone();
        crate::blocking(move || {
            silver.write(&scope, |tx| {
                let mut head = silver_store::head(tx)?
                    .ok_or_else(|| conflict("shared silver has not been indexed"))?;
                head.version = silver_store::token()?;
                silver_store::put_head(tx, &head)
            })
        })
        .await?;
        Ok(Indexed {
            counts: self.status(identity).await?,
            revision: head.revision,
            removed_messages: 0,
            purged_conversations: 0,
        })
    }

    /// Rewrite every document of this node from silver, with vectors from `vector`.
    pub(in crate::shared) async fn write_silver(
        &self,
        identity: &Identity,
        vector: impl Fn(&MessageRecord) -> Option<Vec<f64>>,
    ) -> Result<(), AppError> {
        self.gold.delete_node_documents(identity).await?;
        let mut gold = NodeGold::empty(&self.gold, identity);
        self.gold.put_node_gold(identity, &gold).await?;
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        let build = self.gold.build().clone();
        let mut documents = crate::shared::qdrant_index::Documents::new(identity);
        for (id, fingerprint) in &fingerprints {
            let thread = id.clone();
            let (conversation, messages) = self
                .read(identity, move |connection| {
                    Ok((
                        silver_store::conversation(connection, &thread)?
                            .ok_or_else(|| AppError::internal("silver thread disappeared"))?,
                        silver_store::thread_messages(connection, &thread)?,
                    ))
                })
                .await?;
            let mut written = Written::default();
            for mut message in messages {
                written.ids.push(message.id.clone());
                if build.includes_dense(&message.role, &message.projection) {
                    written.expected += 1;
                    message.vector = vector(&message);
                    message.generation =
                        message.vector.as_ref().map(|_| identity.generation.clone());
                }
                written.embeddings += documents
                    .push(&self.gold, identity, &conversation, message)
                    .await?;
            }
            gold.threads.insert(
                id.clone(),
                GoldThread {
                    fingerprint: fingerprint.clone(),
                    embeddings: written.embeddings,
                    complete: written.embeddings == build.expected_embeddings(written.expected),
                },
            );
        }
        documents.finish(&self.gold).await?;
        self.certify(identity, gold, &fingerprints).await
    }

    pub(in crate::shared) async fn forget(
        &self,
        identity: &Identity,
        input: Forget,
    ) -> Result<Forgotten, AppError> {
        let _node = self.lock(identity).await?;
        self.gold.gold_configuration(identity).await?;
        let previous: Option<Head> = self.head(identity).await?;
        let gold = self.compatible_gold(identity).await?;
        let was_current = gold.as_ref().is_some_and(|g| {
            g.current(
                &self.gold,
                identity,
                previous.as_ref().and_then(|h| h.silver_revision.as_ref()),
            )
        });
        let silver = self.silver.clone();
        let scope = identity.clone();
        let (forgotten, head, deleted) = crate::blocking(move || {
            silver.write(&scope, |tx| {
                silver_store::apply::forget(tx, &scope, &input.instance, input.revision, &input.ids)
            })
        })
        .await?;
        if let Some(mut gold) = gold {
            for id in &deleted {
                self.gold.retain_documents(identity, id, Vec::new()).await?;
                gold.threads.remove(id);
            }
            gold.silver = if was_current {
                head.silver_revision
            } else {
                None
            };
            self.gold.put_node_gold(identity, &gold).await?;
        }
        Ok(Forgotten { forgotten })
    }
}
