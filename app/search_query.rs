//! Application error mapping for the shared pure query contract.
pub(crate) use mash_query::{has_reference, has_terms, is_stop_word, matches_reference, sqlite};

pub(crate) fn validate(query: &str) -> Result<(), crate::AppError> {
    mash_query::validate(query).map_err(|error| crate::AppError::usage(error.to_string()))
}
