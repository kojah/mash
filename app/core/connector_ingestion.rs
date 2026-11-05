//! Local connector ingress: commit silver, then bring local views up to date
//! for the delivered records, as the shared server does. A silver receipt alone
//! never certifies views; `views_ready` reports whether this delivery did.
use crate::AppError;
use crate::config::ResolvedConfig;
use crate::connectors::{Batch, MAX_BATCH_BYTES, Receipt, silver};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) async fn run(
    configuration: &ResolvedConfig,
    database: &Path,
    models: &Path,
    batch_file: PathBuf,
) -> Result<Receipt, AppError> {
    let path = database.to_path_buf();
    let mut receipt = crate::blocking(move || {
        // Validate before creating or opening canonical storage. Input contents
        // never appear in errors or logs, including parser diagnostics.
        let batch = read(&batch_file)?;
        let mut store = silver::Store::open(&path).map_err(silver::app_error)?;
        let receipt = store.apply(&batch).map_err(silver::app_error)?;
        Ok(receipt)
    })
    .await?;
    // Silver is durable; views follow on a best-effort basis. A failure leaves
    // them uncertified until the next delivery or `mash index`.
    match crate::views::local::build(database, models, configuration, false).await {
        Ok(views) => receipt.views_ready = views.ready,
        Err(error) => tracing::warn!(
            kind = ?error.error.kind,
            "local connector views refresh failed; local views stay uncertified"
        ),
    }
    Ok(receipt)
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
