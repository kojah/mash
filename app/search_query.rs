//! View query policy: application error mapping for the shared pure query contract.
pub(crate) use mash_query::{has_reference, has_terms, is_stop_word, matches_reference};

/// Word search has no model, so no language or common-word policy applies.
pub(crate) fn validate_words(query: &str) -> Result<(), crate::AppError> {
    if !has_terms(query) || query.len() > 16384 {
        return Err(crate::AppError::usage(
            "query must contain letters or numbers and be at most 16384 bytes",
        ));
    }
    Ok(())
}

pub(crate) fn validate(query: &str) -> Result<(), crate::AppError> {
    mash_query::validate(query).map_err(|error| crate::AppError::usage(error.to_string()))
}
