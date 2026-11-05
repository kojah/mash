//! Connector silver commits share the authenticated node's database and writer
//! lock; certification brings the node's connector points in line with it.
use super::{Identity, Store};
use crate::storage::connector_views::{self, Evidence, Unit};
use crate::views::materialize::{Writer, connector_digest as digest, connector_point as point};
use crate::views::qdrant::{NodeViews, ProjectionEvidence, ProjectionFamily, ProjectionScope};
use crate::{
    AppError,
    connectors::{Batch, Receipt, silver},
};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// How the server embeds connector units: nodes supply native vectors, but
/// connector records arrive as text.
#[derive(Clone)]
pub(crate) struct Embedder {
    pub(crate) models: PathBuf,
    pub(crate) remote: Option<crate::semantic::remote::Configuration>,
}

/// Units read in one pass: every current digest, and the units that changed.
struct Units {
    digests: BTreeMap<String, String>,
    changed: Vec<(Unit, Evidence)>,
}

impl Store {
    #[cfg(test)]
    pub(crate) fn with_connector_interruption(mut self) -> Self {
        self.connector_interruption = true;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_views_interruption(mut self) -> Self {
        self.views_interruption = true;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_views_partial_interruption(self) -> Self {
        self.views_partial_interruption
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self
    }

    /// Test hook: allow the next views write to certify again.
    #[cfg(test)]
    pub(crate) fn clear_views_partial_interruption(&self) {
        self.views_partial_interruption
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Commit silver first. View follows on a best-effort basis: a failure
    /// leaves the node uncertified until the next ingest, upload or replay.
    pub(crate) async fn ingest_connector(
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
        let known = views.projections.connector_digests();
        let units = self
            .read(identity, move |connection| {
                let mut digests = BTreeMap::new();
                let mut changed = Vec::new();
                for unit in connector_views::silver_units(connection)? {
                    let evidence =
                        connector_views::evidence(connection, &unit.source_id, &unit.record_id)?
                            .ok_or_else(|| AppError::internal("connector object disappeared"))?;
                    let value = digest(&unit, &evidence)?;
                    if known.get(&unit.anchor) != Some(&value) {
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
        let current = units
            .digests
            .iter()
            .map(|(anchor, digest)| {
                (
                    ProjectionScope::ConnectorUnit(anchor.clone()),
                    digest.clone(),
                )
            })
            .collect();
        views
            .projections
            .retain_matching(&current, ProjectionFamily::Connector);
        self.views.put_node_views(identity, views).await?;
        self.views
            .retain_connector_points(identity, units.digests.keys().cloned().collect())
            .await?;
        let model = self
            .embedder
            .as_ref()
            .map(|model| (model.models.as_path(), model.remote.as_ref()));
        let mut writer = Writer::new(&self.views, identity, model);
        for batch in units.changed.chunks(crate::views::shared::BATCH_ROWS) {
            let points = batch
                .iter()
                .map(|(unit, evidence)| point(unit, evidence, None))
                .collect::<Vec<_>>();
            writer.write_connectors(&points).await?;
        }
        writer.finish().await?;
        #[cfg(test)]
        if self.connector_interruption {
            return Err(AppError::internal("injected connector interruption"));
        }
        for (anchor, fingerprint) in units.digests {
            views.projections.insert(
                ProjectionScope::ConnectorUnit(anchor),
                ProjectionEvidence {
                    fingerprint,
                    embeddings: u64::from(self.views.build().dense_enabled()),
                    complete: true,
                },
            );
        }
        Ok(())
    }

    /// Current connector unit digests, for freshness without writing views.
    pub(super) async fn connector_digests(
        &self,
        identity: &Identity,
    ) -> Result<BTreeMap<String, String>, AppError> {
        self.read(identity, |connection| {
            connector_views::silver_units(connection)?
                .iter()
                .map(|unit| {
                    let evidence =
                        connector_views::evidence(connection, &unit.source_id, &unit.record_id)?
                            .ok_or_else(|| AppError::internal("connector object disappeared"))?;
                    Ok((unit.anchor.clone(), digest(unit, &evidence)?))
                })
                .collect()
        })
        .await
    }
}
