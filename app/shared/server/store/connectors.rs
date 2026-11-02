//! Connector silver commits share the authenticated node's database and writer
//! lock; certification brings the node's connector points in line with it.
use super::{Identity, Store};
use crate::shared::qdrant_index::{ConnectorPoint, ConnectorUnit, Documents, NodeViews};
use crate::storage::connector_views::{self, Evidence, Unit};
use crate::{
    AppError,
    connectors::{Batch, Receipt, silver},
};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// How the server embeds connector units: nodes supply native vectors, but
/// connector records arrive as text.
#[derive(Clone)]
pub(in crate::shared) struct Embedder {
    pub(in crate::shared) models: PathBuf,
    pub(in crate::shared) remote: Option<crate::semantic::remote::Configuration>,
}

/// Units read in one pass: every current digest, and the units that changed.
struct Units {
    digests: BTreeMap<String, String>,
    changed: Vec<(Unit, Evidence)>,
}

/// Covers what a point stores beyond the text: kind, group and ancestry.
fn digest(unit: &Unit) -> String {
    blake3::hash(connector_views::unit_digest_input(unit).as_bytes())
        .to_hex()
        .to_string()
}

impl Store {
    #[cfg(test)]
    pub(in crate::shared) fn with_connector_interruption(mut self) -> Self {
        self.connector_interruption = true;
        self
    }

    #[cfg(test)]
    pub(in crate::shared) fn with_views_interruption(mut self) -> Self {
        self.views_interruption = true;
        self
    }

    #[cfg(test)]
    pub(in crate::shared) fn with_views_partial_interruption(self) -> Self {
        self.views_partial_interruption
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self
    }

    /// Test hook: allow the next views write to certify again.
    #[cfg(test)]
    pub(in crate::shared) fn clear_views_partial_interruption(&self) {
        self.views_partial_interruption
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Commit silver first. View follows on a best-effort basis: a failure
    /// leaves the node uncertified until the next ingest, upload or replay.
    pub(in crate::shared) async fn ingest_connector(
        &self,
        identity: &Identity,
        batch: Batch,
    ) -> Result<Receipt, AppError> {
        // An account-scoped source has exactly one writer in a group. Claim it
        // before any silver write, through the durable group-wide table, so
        // both transports inherit the same enforcement point.
        if let Some(claims) = &self.claims {
            let claims = claims.clone();
            let group = identity.group_id.clone();
            let node = identity.node_id.clone();
            let source = batch.source_id.as_str().to_owned();
            let label = source.clone();
            let owner = crate::blocking(move || {
                let catalog = claims
                    .lock()
                    .map_err(|_| AppError::internal("source claim lock is poisoned"))?;
                catalog.claim_source(&group, &source, &node)
            })
            .await?;
            if owner != identity.node_id {
                return Err(crate::shared::validation::conflict(&format!(
                    "source_owned_elsewhere; {label} is owned by {owner}"
                )));
            }
        }
        let _node = self.lock(identity).await?;
        let store = self.silver.clone();
        let scope = identity.clone();
        let receipt = crate::blocking(move || {
            store.write(&scope, |transaction| {
                silver::apply(transaction, &batch).map_err(silver::app_error)
            })
        })
        .await?;
        // A connector delivery certifies synchronously when it can, so the
        // receipt tells the adapter whether its records are searchable yet.
        let mut receipt = receipt;
        match self.recertify(identity).await {
            Ok(()) => receipt.views_ready = true,
            Err(error) => {
                tracing::warn!(
                    kind = ?error.error.kind,
                    "connector views synchronization failed; node views stays uncertified"
                );
            }
        }
        Ok(receipt)
    }

    /// Re-run certification, which also synchronizes connector points.
    async fn recertify(&self, identity: &Identity) -> Result<(), AppError> {
        self.views.views_configuration(identity).await?;
        let views = if let Some(views) = self.compatible_views(identity).await? {
            views
        } else {
            self.views.delete_node_documents(identity).await?;
            NodeViews::empty(&self.views, identity)
        };
        // Re-certification re-reads silver under the node lock, so the chats it
        // keeps are checked against the head it records here.
        let mut views = views;
        views.input = Some(self.head_revision(identity).await?);
        self.certify(
            identity,
            views,
            &crate::shared::activity::Progress::default(),
        )
        .await
    }

    /// Bring this node's connector points in line with its shared silver.
    /// Only changed units are embedded and written; removed ones are deleted.
    pub(super) async fn sync_connectors(
        &self,
        identity: &Identity,
        views: &mut NodeViews,
    ) -> Result<(), AppError> {
        let known = views.connectors.clone();
        let units = self
            .read(identity, move |connection| {
                let mut digests = BTreeMap::new();
                let mut changed = Vec::new();
                for unit in connector_views::silver_units(connection)? {
                    let value = digest(&unit);
                    if known.get(&unit.anchor) != Some(&value) {
                        let evidence = connector_views::evidence(
                            connection,
                            &unit.source_id,
                            &unit.record_id,
                        )?
                        .ok_or_else(|| AppError::internal("connector object disappeared"))?;
                        changed.push((unit.clone(), evidence));
                    }
                    digests.insert(unit.anchor, value);
                }
                Ok(Units { digests, changed })
            })
            .await?;
        // Points no longer match the recorded digests until this completes.
        views.certified = false;
        views.silver = None;
        // Reversion after a failed write must not reuse the old digest: the
        // physical point may have been overwritten or deleted in the meantime.
        views
            .connectors
            .retain(|anchor, digest| units.digests.get(anchor) == Some(digest));
        self.views.put_node_views(identity, views).await?;
        self.views
            .retain_connector_points(identity, units.digests.keys().cloned().collect())
            .await?;
        let mut documents = Documents::new(identity);
        let mut pool = None;
        let mut build = crate::views::shared::Build::default();
        for batch in units.changed.chunks(crate::views::shared::BATCH_ROWS) {
            let vectors = if self.views.build().dense_enabled() {
                let embedder = self.embedder.as_ref().ok_or_else(|| {
                    AppError::model(
                        "the shared server has no embedding model for connector records",
                    )
                })?;
                let texts = batch
                    .iter()
                    .map(|(unit, _)| unit.text.as_str())
                    .collect::<Vec<_>>();
                let vectors = crate::views::shared::embed_texts(
                    &embedder.models,
                    embedder.remote.as_ref(),
                    &mut pool,
                    &texts,
                    &mut build,
                )
                .await?;
                if vectors.len() != texts.len() {
                    return Err(AppError::model("missing connector embedding"));
                }
                vectors.into_iter().map(Some).collect()
            } else {
                vec![None; batch.len()]
            };
            for ((unit, evidence), vector) in batch.iter().zip(vectors) {
                documents
                    .push_connector(&self.views, identity, point(unit, evidence, vector))
                    .await?;
            }
        }
        documents.finish(&self.views).await?;
        #[cfg(test)]
        if self.connector_interruption {
            return Err(AppError::internal("injected connector interruption"));
        }
        views.connectors = units.digests;
        Ok(())
    }

    /// Current connector unit digests, for freshness without writing views.
    pub(super) async fn connector_digests(
        &self,
        identity: &Identity,
    ) -> Result<BTreeMap<String, String>, AppError> {
        self.read(identity, |connection| {
            Ok(connector_views::silver_units(connection)?
                .iter()
                .map(|unit| (unit.anchor.clone(), digest(unit)))
                .collect())
        })
        .await
    }
}

fn point(unit: &Unit, evidence: &Evidence, vector: Option<Vec<f64>>) -> ConnectorPoint {
    let (group_created_at, group_updated_at) = evidence.group_times();
    ConnectorPoint {
        source_id: unit.source_id.clone(),
        object: unit.object.clone(),
        group_anchor: unit.group_anchor.clone(),
        created_at: unit.created_at,
        unit: ConnectorUnit {
            anchor: unit.anchor.clone(),
            text: unit.text.clone(),
            content: evidence.content().to_owned(),
            title: evidence.title().map(str::to_owned),
            author_kind: evidence.author(),
            sequence: unit.sequence,
            chat_created_at: group_created_at,
            chat_updated_at: group_updated_at,
            ancestors: evidence.ancestors(&unit.source_id),
            generation: None,
        },
        vector,
    }
}
