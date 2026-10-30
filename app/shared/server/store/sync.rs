//! Commit silver first, then bring views in line with it. A failed views update
//! leaves views marked stale; the next snapshot or a replay repairs it.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::activity::{Phase, Progress, Unit};
use crate::shared::protocol::{Batch, ConversationRecord, Event, Indexed};
use crate::shared::qdrant_index::{NodeViews, ViewChat};
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
    #[cfg(test)]
    pub(in crate::shared) async fn index(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        self.index_tracked(identity, file, Progress::default())
            .await
    }

    #[tracing::instrument(name = "backend_index", skip_all, fields(backend = "sqlite+qdrant"))]
    pub(in crate::shared) async fn index_tracked(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
        progress: Progress,
    ) -> Result<Indexed, AppError> {
        progress.phase(Phase::WaitingForNode, Unit::Records);
        let _node = self.lock(identity).await?;
        progress.phase(Phase::CheckingViews, Unit::Records);
        self.views.views_configuration(identity).await?;
        let previous = self.head(identity).await?;
        let views = self.compatible_views(identity).await?;
        let silver_revision = previous.as_ref().and_then(|h| h.silver_revision.clone());
        let current = views
            .as_ref()
            .is_some_and(|g| g.current(&self.views, identity, silver_revision.as_ref()));
        let silver = self.silver.clone();
        let scope = identity.clone();
        let build = self.views.build().clone();
        let worker_progress = progress.clone();
        progress.phase(Phase::ApplyingSilver, Unit::Records);
        let (applied, file) = crate::blocking(move || {
            let applied =
                silver_store::apply::write(&silver, &scope, &file, &build, &worker_progress)?;
            Ok((applied, file))
        })
        .await?;
        if !(applied.duplicate && current) {
            self.sync_upload(identity, &file, &applied, views, &progress)
                .await?;
        }
        Ok(Indexed {
            counts: self.status(identity).await?,
            revision: applied.revision,
            removed_messages: applied.removed,
            purged_conversations: applied.purged,
        })
    }

    /// Write views for the chats an upload supplied, then certify the revision.
    async fn sync_upload(
        &self,
        identity: &Identity,
        file: &tempfile::NamedTempFile,
        applied: &Applied,
        previous: Option<NodeViews>,
        progress: &Progress,
    ) -> Result<(), AppError> {
        progress.phase(Phase::WritingViews, Unit::Records);
        // Without prior views every document is deleted, so none can be stale.
        let fresh = previous.is_none();
        let mut views = if let Some(views) = previous {
            views
        } else {
            self.views.delete_node_documents(identity).await?;
            NodeViews::empty(&self.views, identity)
        };
        views.silver = None;
        self.views.put_node_views(identity, &views).await?;
        for id in &applied.deleted {
            self.views
                .retain_documents(identity, id, Vec::new())
                .await?;
            views.chats.remove(id);
        }
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        // An earlier attempt may have committed silver or written points and
        // then failed before updating metadata. Its deleted set is not durable.
        self.views
            .retain_node_conversations(identity, fingerprints.keys().cloned().collect())
            .await?;
        // A duplicate snapshot resupplies every chat with its vectors.
        let supplied: BTreeSet<String> = if applied.duplicate {
            fingerprints.keys().cloned().collect()
        } else {
            applied.supplied.clone()
        };
        let written = self
            .write_upload(identity, file, &supplied, progress)
            .await?;
        for id in &supplied {
            let chat = written.get(id);
            if !fresh {
                let current = chat.map(|t| t.ids.clone()).unwrap_or_default();
                self.views.retain_documents(identity, id, current).await?;
            }
            let fingerprint = fingerprints
                .get(id)
                .cloned()
                .ok_or_else(|| AppError::internal("supplied chat has no silver"))?;
            views.chats.insert(
                id.clone(),
                ViewChat {
                    fingerprint,
                    embeddings: chat.map_or(0, |t| t.embeddings),
                    complete: chat.is_none_or(|t| t.embeddings == t.expected),
                },
            );
        }
        progress.phase(Phase::CertifyingViews, Unit::Chats);
        self.certify(identity, views, &fingerprints, progress).await
    }

    /// Stream the upload's messages for the selected chats into documents.
    async fn write_upload(
        &self,
        identity: &Identity,
        file: &tempfile::NamedTempFile,
        selected: &BTreeSet<String>,
        progress: &Progress,
    ) -> Result<BTreeMap<String, Written>, AppError> {
        let mut lines = std::io::BufReader::new(file.reopen().map_err(AppError::io)?).lines();
        let mut written = BTreeMap::<String, Written>::new();
        let mut conversations = HashMap::<String, ConversationRecord>::new();
        let build = self.views.build().clone();
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
                    .push(&self.views, identity, &conversations[&id], message)
                    .await?;
                progress.advance(1);
            }
        }
        documents.finish(&self.views).await?;
        Ok(written)
    }

    /// Record views as current only when every silver chat has complete documents.
    pub(in crate::shared) async fn certify(
        &self,
        identity: &Identity,
        mut views: NodeViews,
        fingerprints: &BTreeMap<String, String>,
        progress: &Progress,
    ) -> Result<(), AppError> {
        self.sync_connectors(identity, &mut views).await?;
        views.chats.retain(|id, _| fingerprints.contains_key(id));
        let head = self.head(identity).await?;
        let synced = views.chats.len() == fingerprints.len()
            && fingerprints.iter().all(|(id, fingerprint)| {
                views
                    .chats
                    .get(id)
                    .is_some_and(|t| &t.fingerprint == fingerprint)
            });
        views.certified = synced && views.chats.values().all(|chat| chat.complete);
        views.silver = synced
            .then(|| head.and_then(|h| h.silver_revision))
            .flatten();
        self.views.put_node_views(identity, &views).await?;
        self.views.settle(identity, progress).await
    }
}
