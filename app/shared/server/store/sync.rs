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

/// Silver committed and awaiting views certification. Holds the node lock so
/// no other publication interleaves between the two phases.
pub(in crate::shared) struct SilverCommit {
    identity: Identity,
    file: tempfile::NamedTempFile,
    applied: Applied,
    views: Option<NodeViews>,
    current: bool,
    progress: Progress,
    _node: tokio::sync::OwnedMutexGuard<()>,
}

impl SilverCommit {
    /// Source revision committed in silver.
    pub(in crate::shared) fn revision(&self) -> i64 {
        self.applied.revision
    }
}

impl Store {
    /// Test helper: commit silver and certify views in one call.
    #[cfg(test)]
    pub(in crate::shared) async fn index(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
    ) -> Result<Indexed, AppError> {
        let commit = self
            .commit_silver(identity, file, Progress::default(), None, false)
            .await?;
        self.certify_views(commit).await
    }

    /// First phase: atomically commit canonical silver, its receipt and the
    /// publication head. Views are untouched, so a caller can report
    /// `SilverCommitted` before certification starts.
    pub(in crate::shared) async fn commit_silver(
        &self,
        identity: &Identity,
        file: tempfile::NamedTempFile,
        progress: Progress,
        receipt: Option<&silver_store::SyncReceipt>,
        rebase: bool,
    ) -> Result<SilverCommit, AppError> {
        progress.phase(Phase::WaitingForNode, Unit::Records);
        let node = self.lock(identity).await?;
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
        let receipt = receipt.cloned();
        progress.phase(Phase::ApplyingSilver, Unit::Records);
        let (applied, file) = crate::blocking(move || {
            let applied = silver_store::apply::write(
                &silver,
                &scope,
                &file,
                &build,
                &worker_progress,
                receipt.as_ref(),
                rebase,
            )?;
            Ok((applied, file))
        })
        .await?;
        Ok(SilverCommit {
            identity: identity.clone(),
            file,
            applied,
            views,
            current,
            progress,
            _node: node,
        })
    }

    /// Second phase: write and certify views for a committed silver revision.
    pub(in crate::shared) async fn certify_views(
        &self,
        commit: SilverCommit,
    ) -> Result<Indexed, AppError> {
        let SilverCommit {
            identity,
            file,
            applied,
            views,
            current,
            progress,
            _node: node,
        } = commit;
        // Silver ordering is settled by the commit above; certification writes
        // views and must not block another batch's commit for this node.
        drop(node);
        if !(applied.duplicate && current) {
            #[cfg(test)]
            if self.views_interruption {
                return Err(AppError::internal("injected views certification failure"));
            }
            self.sync_upload(&identity, &file, &applied, views, &progress)
                .await?;
        }
        Ok(Indexed {
            counts: self.status(&identity).await?,
            revision: applied.revision,
            removed_messages: applied.removed,
            purged_conversations: applied.purged,
            views_current: self.views_current(&identity).await?,
        })
    }

    /// Whether this node's stored views cover the current head. Certification
    /// can complete successfully without claiming the head when a newer commit
    /// landed first, so the receipt must ask instead of assuming.
    pub(in crate::shared) async fn views_current(
        &self,
        identity: &Identity,
    ) -> Result<bool, AppError> {
        let head = self.head(identity).await?;
        let silver = head.as_ref().and_then(|head| head.silver_revision.as_ref());
        Ok(self
            .views
            .node_views(identity)
            .await?
            .is_some_and(|views| views.current(&self.views, identity, silver)))
    }

    /// Current accepted source revision for this node; 0 before its first
    /// publication. Baseline batches rebase onto it so per-conversation
    /// ordering, not node-level staleness, decides what applies.
    pub(in crate::shared) async fn head_revision(
        &self,
        identity: &Identity,
    ) -> Result<i64, AppError> {
        Ok(self.head(identity).await?.map_or(0, |head| head.revision))
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
        #[cfg(test)]
        if self
            .views_partial_interruption
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            // Documents were written; certification never runs. A retry must
            // rewrite and certify rather than trust the stale metadata.
            return Err(AppError::internal(
                "injected views failure after document writes",
            ));
        }
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
        // The revision these views were built from, not the head reached by the
        // time certification finishes; the guard below compares this.
        views.input = Some(applied.revision);
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
        if let Some(current) = self.views.node_views(identity).await?
            && let (Some(current), Some(incoming)) = (current.input, views.input)
            && current > incoming
        {
            // A newer certification already wrote these chats. Writing ours
            // would stamp the newer head over older content.
            return Ok(());
        }
        self.sync_connectors(identity, &mut views).await?;
        views.chats.retain(|id, _| fingerprints.contains_key(id));
        let head = self.head(identity).await?;
        let head_revision = head.as_ref().map(|head| head.revision);
        // Every caller records the revision its chats were built from. Treat a
        // missing input as a bug rather than silently claiming the head: the
        // receipt would otherwise assert content it cannot prove.
        let input = views
            .input
            .ok_or_else(|| AppError::internal("certification requires an input revision"))?;
        let synced = views.chats.len() == fingerprints.len()
            && fingerprints.iter().all(|(id, fingerprint)| {
                views
                    .chats
                    .get(id)
                    .is_some_and(|t| &t.fingerprint == fingerprint)
            });
        // A certification that finishes after a newer commit wrote every chat
        // of its own revision, but not of the head. Stamping the head would make
        // searches serve older chats as current, so it only claims the revision
        // it was built from.
        let current = head_revision == Some(input);
        views.certified = synced && current && views.chats.values().all(|chat| chat.complete);
        views.silver = current
            .then(|| head.and_then(|head| head.silver_revision))
            .flatten();
        // The guard above ran before the slow connector synchronization, so a
        // newer certification can land in between. Re-check under the write
        // lock: an older certification must never replace a newer one.
        let write = self.views_write_guard(identity).await?;
        if let Some(latest) = self.views.node_views(identity).await?
            && let (Some(latest), Some(incoming)) = (latest.input, views.input)
            && latest > incoming
        {
            return Ok(());
        }
        self.views.put_node_views(identity, &views).await?;
        drop(write);
        self.views.settle(identity, progress).await
    }
}
