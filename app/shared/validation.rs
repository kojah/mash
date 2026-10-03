//! Shared identity and conflict validation, independent of storage transport.
use crate::AppError;

pub(super) fn conflict(message: &str) -> AppError {
    let mut error = AppError::shared(message);
    error.error.kind = crate::ErrorKind::Conflict;
    error
}

pub(super) fn valid_id(id: &str) -> Result<(), AppError> {
    if id.is_empty() || id.len() > 4096 {
        Err(AppError::usage("invalid record identity"))
    } else {
        Ok(())
    }
}
