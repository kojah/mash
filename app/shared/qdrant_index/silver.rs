//! Serialize one captured immutable silver snapshot, without cloning text bodies.
use super::{Identity, Store};
use crate::{
    AppError,
    shared::{gold_rebuild, silver_read, validation::conflict},
};
use tokio::sync::OwnedSemaphorePermit;

impl Store {
    pub(in crate::shared) async fn publication_plan(
        &self,
        identity: &Identity,
        permit: OwnedSemaphorePermit,
    ) -> Result<(tempfile::NamedTempFile, OwnedSemaphorePermit), AppError> {
        if self.build == crate::gold::config::Build::default() {
            // Default snapshots retain local vectors. Their negotiation needs
            // only the small head, without decoding/fingerprinting shared history.
            let node = self.node(identity).await?;
            let target = crate::shared::reuse::Target::metadata(
                node.head.as_ref(),
                &node.version,
                identity,
                self.build.clone(),
            );
            return crate::blocking(move || spool_target(&target, permit)).await;
        }
        let pending = self.pending_snapshot(identity).await?;
        let reservation = self.snapshot_budget.reserve(pending.encoded_bytes(), 0)?;
        let identity = identity.clone();
        let build = self.build.clone();
        crate::blocking(move || {
            let _reservation = reservation;
            let target = crate::shared::reuse::Target::new(&pending.decode()?, &identity, build)?;
            spool_target(&target, permit)
        })
        .await
    }

    pub(in crate::shared) async fn silver(
        &self,
        identity: &Identity,
        permit: OwnedSemaphorePermit,
    ) -> Result<(tempfile::NamedTempFile, OwnedSemaphorePermit), AppError> {
        let pending = self.pending_snapshot(identity).await?;
        let reservation = self.snapshot_budget.reserve(pending.encoded_bytes(), 0)?;
        let generation = identity.generation.clone();
        let build = self.build.clone();
        let dimensions = identity.dimensions;
        crate::blocking(move || {
            let _reservation = reservation;
            let loaded = pending.decode()?;
            if !gold_rebuild::certified(&loaded) {
                return Err(conflict(
                    "gold rebuild requires complete, current shared silver",
                ));
            }
            let head = loaded
                .head
                .as_ref()
                .ok_or_else(|| conflict("shared silver has not been indexed"))?;
            let silver = head.silver_revision.as_ref().ok_or_else(|| {
                conflict("shared silver identity is unverified; run explicit indexing")
            })?;
            silver.validate()?;
            let mut file = tempfile::NamedTempFile::new().map_err(AppError::io)?;
            let mut output = std::io::BufWriter::with_capacity(64 * 1024, file.as_file_mut());
            serde_json::to_writer(
                &mut output,
                &silver_read::Snapshot {
                    build: &build,
                    publication_revision: head.revision,
                    silver,
                    generation: &generation,
                    dimensions,
                    history: loaded.data.silver(),
                },
            )
            .map_err(|_| AppError::internal("serialize retained shared silver"))?;
            std::io::Write::flush(&mut output).map_err(AppError::io)?;
            drop(output);
            Ok((file, permit))
        })
        .await
    }
}

fn spool_target(
    target: &crate::shared::reuse::Target,
    permit: OwnedSemaphorePermit,
) -> Result<(tempfile::NamedTempFile, OwnedSemaphorePermit), AppError> {
    let mut file = tempfile::NamedTempFile::new().map_err(AppError::io)?;
    let mut output = std::io::BufWriter::with_capacity(64 * 1024, file.as_file_mut());
    serde_json::to_writer(&mut output, target)
        .map_err(|_| AppError::internal("serialize shared publication plan"))?;
    std::io::Write::flush(&mut output).map_err(AppError::io)?;
    drop(output);
    Ok((file, permit))
}
