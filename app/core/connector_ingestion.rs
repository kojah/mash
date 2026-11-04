//! Local connector ingress: commit silver, then bring local views up to date
//! for the delivered records, as the shared server does. A silver receipt alone
//! never certifies views; `views_ready` reports whether this delivery did.
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::connectors::{Batch, MAX_BATCH_BYTES, Receipt, silver};
use crate::semantic::Models;
use crate::storage::Storage;
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) async fn run(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    batch_file: PathBuf,
) -> Result<Receipt, AppError> {
    let path = database.to_path_buf();
    let (mut receipt, before, batch) = crate::blocking(move || {
        // Validate before creating or opening canonical storage. Input contents
        // never appear in errors or logs, including parser diagnostics.
        let batch = read(&batch_file)?;
        let mut store = silver::Store::open(&path).map_err(silver::app_error)?;
        let before = store.silver_revision().map_err(silver::app_error)?;
        let receipt = store.apply(&batch).map_err(silver::app_error)?;
        Ok((receipt, before, batch))
    })
    .await?;
    // Silver is durable; views follow on a best-effort basis. A failure leaves
    // them uncertified until the next delivery or `mash index`.
    let database = database.to_path_buf();
    let models = Models::new(models.to_path_buf()).with_remote(configuration.embeddings.clone());
    let build = configuration.views.build.clone();
    match crate::blocking(move || refresh_views(&database, &models, &build, before, &batch)).await {
        Ok(ready) => receipt.views_ready = ready,
        Err(error) => tracing::warn!(
            kind = ?error.error.kind,
            "local connector views refresh failed; local views stay uncertified"
        ),
    }
    Ok(receipt)
}

/// Update the published views to the new silver revision when only that
/// changed: connector units are re-synchronized by digest and only new units
/// are embedded. Anything that needs a full rebuild (a new recipe or model,
/// dirty native state, an unfinished native scan) is left to `mash index`,
/// rather than rebuilt once per delivered batch.
fn refresh_views(
    database: &Path,
    models: &Models,
    build: &crate::views::config::Build,
    before: u64,
    batch: &Batch,
) -> Result<bool, AppError> {
    let mut storage = Storage::open_writer(database)?;
    let model = models.generation();
    let Some(previous) = storage.views_manifest()? else {
        return Ok(false);
    };
    let incremental = storage.silver_certificate()?.permits_views()
        && !storage.derived_search_is_dirty()?
        && &previous.build == build
        && previous.model_generation == model;
    if !incremental || (build.dense_enabled() && models.require_indexer().is_err()) {
        return Ok(false);
    }
    // When views were current just before this batch, only connector silver
    // changed since, and only in the conversations it touched; otherwise
    // native state is rechecked and every connector unit compared.
    let connector_only = previous.silver.revision == before
        && previous.matches(
            build,
            &model,
            &crate::silver::certificate::Certificate {
                revision: before,
                ..storage.silver_certificate()?
            },
        );
    if connector_only && let Some(scope) = storage.connector_conversations_touched(batch)? {
        storage.scope_connector_refresh(scope);
    }
    let views = crate::views::local::build(
        &mut storage,
        models,
        &crate::silver::IndexSummary::default(),
        build,
        connector_only,
    )?;
    storage.commit_writer()?;
    Ok(views.ready)
}

fn read(path: &Path) -> Result<Batch, AppError> {
    let metadata = std::fs::metadata(path).map_err(AppError::io)?;
    if !metadata.is_file() || metadata.len() > MAX_BATCH_BYTES as u64 {
        return Err(AppError::usage(
            "connector batch requires a bounded regular file",
        ));
    }
    let file = std::fs::File::open(path).map_err(AppError::io)?;
    if !file.metadata().map_err(AppError::io)?.is_file() {
        return Err(AppError::usage("connector batch requires a regular file"));
    }
    let mut bytes = Vec::new();
    file.take((MAX_BATCH_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(AppError::io)?;
    Batch::decode(&bytes).map_err(|problem| silver::app_error(silver::Error::InvalidBatch(problem)))
}
