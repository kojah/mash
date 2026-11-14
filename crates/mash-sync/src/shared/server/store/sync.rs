//! Commit silver first, then bring views in line with it. A failed views update
//! leaves views marked stale; the next snapshot or a replay repairs it.
use super::{Identity, Store};
use crate::AppError;
use crate::shared::activity::{Phase, Progress, Unit};
use crate::shared::protocol::{Batch, ChatRecord, Event, Indexed};
use crate::shared::silver_store::{self, Applied};
use crate::views::qdrant::{NodeViews, ProjectionEvidence};
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
pub struct SilverCommit {
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
    pub fn revision(&self) -> i64 {
        self.applied.revision
    }
}

impl Store {
    /// Test helper: commit silver and certify views in one call.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn index(
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
    pub async fn commit_silver(
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
        // Chat reads come from silver, so listeners can reread as soon as it
        // commits; they need not wait for views.
        if !applied.duplicate {
            self.events.announce(
                identity,
                applied.revision,
                applied.supplied.iter().chain(&applied.deleted).cloned(),
                applied.records,
            );
        }
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
    pub async fn certify_views(&self, commit: SilverCommit) -> Result<Indexed, AppError> {
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
            #[cfg(any(test, feature = "test-support"))]
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
            purged_chats: applied.purged,
            views_current: self.views_current(&identity).await?,
        })
    }

    /// Whether this node's stored views cover the current head. Certification
    /// can complete successfully without claiming the head when a newer commit
    /// landed first, so the receipt must ask instead of assuming.
    pub async fn views_current(&self, identity: &Identity) -> Result<bool, AppError> {
        let head = self.head(identity).await?;
        let silver = head.as_ref().and_then(|head| head.silver_revision.as_ref());
        Ok(self
            .views
            .node_views(identity)
            .await?
            .is_some_and(|views| views.current(&self.views, identity, silver)))
    }

    /// Current accepted source revision for this node; 0 before its first
    /// publication. Baseline batches rebase onto it so per-chat
    /// ordering, not node-level staleness, decides what applies.
    pub async fn head_revision(&self, identity: &Identity) -> Result<i64, AppError> {
        Ok(self.head(identity).await?.map_or(0, |head| head.revision))
    }

    /// Record that the node checked in with history the server already holds:
    /// replaying its current receipt advances `last_synced_at` like a duplicate
    /// publication, without changing silver. One immediate transaction keeps
    /// the read-modify-write ordered against concurrent commits.
    pub async fn record_check_in(&self, identity: &Identity) -> Result<(), AppError> {
        let silver = self.silver.clone();
        let identity = identity.clone();
        crate::blocking(move || {
            silver.write(&identity, |transaction| {
                if let Some(mut head) = crate::shared::silver_store::head(transaction)? {
                    head.last_synced_at = Some(crate::shared::silver_store::now());
                    crate::shared::silver_store::put_head(transaction, &head)?;
                }
                Ok(())
            })
        })
        .await
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
        // Another certification, such as a parallel baseline lane, may have
        // created compatible views since this commit found none; starting
        // fresh would delete the documents it wrote.
        let previous = match previous {
            Some(previous) => Some(previous),
            None => self.compatible_views(identity).await?,
        };
        // Without prior views every document is deleted, so none can be stale.
        let fresh = previous.is_none();
        let mut views = if let Some(views) = previous {
            views
        } else {
            self.views.delete_node_documents(identity).await?;
            NodeViews::empty(&self.views, identity)
        };
        views.silver = None;
        {
            // Mark the views uncertified without dropping chats another
            // certification recorded since this commit read them.
            let _write = self.views_write_guard(identity).await?;
            if !fresh
                && let Some(latest) = self.views.node_views(identity).await?
                && latest.compatible(&self.views, identity)
            {
                // This upload has not written any new coverage yet. The
                // latest stored state supersedes the commit-time snapshot;
                // keeping its old markers can erase a completed batch's
                // replacement before the certification merge sees it.
                views = latest;
            }
            views.silver = None;
            self.views.put_node_views(identity, &views).await?;
        }
        for id in &applied.deleted {
            self.views
                .retain_documents(identity, id, Vec::new())
                .await?;
            views.projections.remove(&id.clone());
        }
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        // An earlier attempt may have committed silver or written points and
        // then failed before updating metadata. Its deleted set is not durable.
        self.views
            .retain_node_chats(identity, fingerprints.keys().cloned().collect())
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
        #[cfg(any(test, feature = "test-support"))]
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
            let fingerprint = fingerprints
                .get(id)
                .cloned()
                .ok_or_else(|| AppError::internal("supplied chat has no silver"))?;
            let complete = chat.is_none_or(|t| t.embeddings == t.expected);
            let embeddings = chat.map_or(0, |t| t.embeddings);
            if let Some(amended) = applied.amended.get(id)
                && amended.prior.is_some()
            {
                let evidence = ProjectionEvidence {
                    fingerprint,
                    embeddings,
                    complete,
                };
                self.amend_views(identity, id, amended, chat, evidence, fresh, &mut views)
                    .await?;
                continue;
            }
            if !fresh {
                let current = chat.map_or_else(Default::default, |t| t.ids.clone());
                self.views.retain_documents(identity, id, current).await?;
            }
            views.projections.insert(
                id.clone(),
                ProjectionEvidence {
                    fingerprint,
                    embeddings,
                    complete,
                },
            );
        }
        progress.phase(Phase::CertifyingViews, Unit::Chats);
        // The revision these views were built from, not the head reached by the
        // time certification finishes; the guard below compares this.
        views.input = Some(applied.revision);
        self.certify(identity, views, progress).await
    }

    /// The upload carries only an amended chat's changed messages; its other
    /// documents stay. Remove those it replaced without a successor, and carry
    /// the evidence forward only from views built for exactly the chat this
    /// upload amended. `written` is the evidence of the supplied messages.
    #[allow(
        clippy::too_many_arguments,
        reason = "one chat's amendment, its written documents and the views being certified"
    )]
    async fn amend_views(
        &self,
        identity: &Identity,
        id: &str,
        amended: &silver_store::apply::Amended,
        chat: Option<&Written>,
        written: ProjectionEvidence,
        fresh: bool,
        views: &mut NodeViews,
    ) -> Result<(), AppError> {
        let rewritten: BTreeSet<&String> = chat.map_or_default(|t| t.ids.iter().collect());
        let stale: Vec<String> = amended
            .replaced
            .iter()
            .filter(|message| !rewritten.contains(message))
            .cloned()
            .collect();
        if !fresh {
            self.views.remove_documents(identity, id, stale).await?;
        }
        match views.projections.get(id) {
            Some(previous) if !fresh && Some(&previous.fingerprint) == amended.prior.as_ref() => {
                let evidence = ProjectionEvidence {
                    fingerprint: written.fingerprint,
                    embeddings: previous
                        .embeddings
                        .saturating_sub(amended.removed_embeddings)
                        .saturating_add(written.embeddings),
                    complete: previous.complete && written.complete,
                };
                views.projections.insert(id.to_owned(), evidence);
            }
            // Nothing vouches for the documents this upload kept, so the chat
            // stays uncovered until it is resupplied whole.
            _ => views.projections.remove(id),
        }
        Ok(())
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
        let mut chats = HashMap::<String, ChatRecord>::new();
        let build = self.views.build().clone();
        let mut documents = crate::views::qdrant::Documents::new(identity);
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
                let id = message.chat_id.clone();
                if !selected.contains(&id) {
                    continue;
                }
                if !chats.contains_key(&id) {
                    let chat = id.clone();
                    let record = self
                        .read(identity, move |connection| {
                            silver_store::chat(connection, &chat)
                        })
                        .await?
                        .ok_or_else(|| AppError::internal("supplied chat has no silver"))?;
                    chats.insert(id.clone(), record);
                }
                let entry = written.entry(id.clone()).or_default();
                if !message.projection.is_empty() {
                    entry.ids.push(message.id.clone());
                }
                entry.expected +=
                    u64::from(build.includes_dense(&message.role, &message.projection));
                entry.embeddings += documents
                    .push(&self.views, identity, &chats[&id], message)
                    .await?;
                progress.advance(1);
            }
        }
        documents.finish(&self.views).await?;
        Ok(written)
    }

    /// Record views as current only when every silver chat has complete documents.
    pub async fn certify(
        &self,
        identity: &Identity,
        views: NodeViews,
        progress: &Progress,
    ) -> Result<(), AppError> {
        self.certify_inner(identity, views, progress, false).await
    }

    /// Certification for a bulk replay; the final state write is the barrier,
    /// then the group-wide graph threshold is restored once.
    pub async fn certify_bulk(
        &self,
        identity: &Identity,
        views: NodeViews,
        progress: &Progress,
    ) -> Result<(), AppError> {
        self.certify_inner(identity, views, progress, true).await
    }

    async fn certify_inner(
        &self,
        identity: &Identity,
        mut views: NodeViews,
        progress: &Progress,
        bulk: bool,
    ) -> Result<(), AppError> {
        // Every caller records the revision its chats were built from. Treat a
        // missing input as a bug rather than silently claiming the head: the
        // receipt would otherwise assert content it cannot prove.
        views
            .input
            .ok_or_else(|| AppError::internal("certification requires an input revision"))?;
        // Decide under the write lock, against the stored views and silver as
        // they are now: concurrent certifications (parallel baseline lanes)
        // each build only their own chats and may finish in any order.
        let write = self.views_write_guard(identity).await?;
        let latest = self.views.node_views(identity).await?;
        let fingerprints = self
            .read(identity, |connection| {
                silver_store::fingerprints(connection)
            })
            .await?;
        let compatible = |candidate: &NodeViews| candidate.compatible(&self.views, identity);
        let mut views = match latest {
            // A newer certification is stored: never replace it, but add the
            // chats this one built that it lacks, so neither finishing order
            // loses coverage. An older one must not stamp its revision.
            Some(mut latest) if latest.input > views.input => {
                if compatible(&latest) && compatible(&views) {
                    latest
                        .projections
                        .merge_matching(&views.projections, &fingerprints);
                }
                latest
            }
            // Chats an earlier certification recorded count when built with
            // this recipe for the same content; a recipe change resets the
            // stored views, so its stale chats never do.
            Some(latest) => {
                if compatible(&latest) {
                    views
                        .projections
                        .merge_matching(&latest.projections, &fingerprints);
                }
                views
            }
            None => views,
        };
        let current = fingerprints
            .iter()
            .map(|(id, fingerprint)| (id.clone(), fingerprint.clone()))
            .collect();
        views.projections.retain_matching(&current);
        let head = self.head(identity).await?;
        let synced = views.projections.chats().count() == fingerprints.len();
        // A certification that finishes after a newer commit wrote every chat
        // of its own revision, but not of the head. Stamping the head would make
        // searches serve older chats as current, so it only claims the revision
        // it was built from.
        let current = head.as_ref().map(|head| head.revision) == views.input;
        views.certified = synced && current && views.projections.complete();
        // Claim a silver revision only when these views cover every chat of
        // it (`synced`) and were built from it (`current`); either alone lets
        // a partial rebuild, such as one after a recipe change, pass as current.
        views.silver = (synced && current)
            .then(|| head.and_then(|head| head.silver_revision))
            .flatten();
        self.views.put_node_views(identity, &views).await?;
        drop(write);
        if bulk {
            self.views.end_bulk(identity).await?;
        }
        progress.phase(
            crate::shared::activity::Phase::CheckingSearch,
            crate::shared::activity::Unit::Records,
        );
        self.views.settle(identity).await
    }
}
