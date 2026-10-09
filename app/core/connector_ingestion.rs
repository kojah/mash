//! Local connector ingress. Silver acknowledgement does not certify gold.
use crate::AppError;
use crate::connectors::{Batch, MAX_BATCH_BYTES, Receipt, silver};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) async fn run(database: &Path, batch_file: PathBuf) -> Result<Receipt, AppError> {
    let database = database.to_path_buf();
    crate::blocking(move || {
        // Validate before creating or opening canonical storage. Input contents
        // never appear in errors or logs, including parser diagnostics.
        let batch = read(&batch_file)?;
        silver::Store::open(&database)
            .and_then(|mut store| store.apply(&batch))
            .map_err(error)
    })
    .await
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
    Batch::decode(&bytes).map_err(|problem| error(silver::Error::InvalidBatch(problem)))
}

fn error(problem: silver::Error) -> AppError {
    use silver::Error;
    match problem {
        Error::BatchConflict | Error::RevisionConflict => {
            AppError::new(12, crate::ErrorKind::Conflict, problem.to_string(), false)
        }
        Error::InvalidBatch(_) | Error::InvalidReferences => AppError::usage(problem.to_string()),
        Error::UnsupportedSchema => AppError::schema(problem.to_string()),
        Error::Database => AppError::new(5, crate::ErrorKind::Database, problem.to_string(), true),
        Error::CorruptRecord | Error::RevisionExhausted => {
            AppError::database_data(problem.to_string())
        }
    }
}
