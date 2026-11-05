//! Connector silver commits share the authenticated node's database and writer
//! lock; certification brings the node's connector points in line with it.
use super::{Identity, Store};
use crate::AppError;
use crate::storage::connector_views::{self, Evidence, Unit};
use crate::views::materialize::{Writer, connector_digest as digest, connector_point as point};
use crate::views::qdrant::{NodeViews, ProjectionEvidence, ProjectionFamily, ProjectionScope};
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
