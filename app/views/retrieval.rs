//! Retrieval requests and candidates shared by direct and HTTP callers.
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SearchRequest")]
pub struct Search {
    #[serde(default)]
    pub match_mode: crate::views::matching::Mode,
    #[serde(
        default,
        rename = "vector_search",
        skip_serializing_if = "Option::is_none"
    )]
    pub vector_policy: Option<crate::views::vector::Search>,
    #[serde(default, skip_serializing_if = "related_discussion")]
    pub intent: crate::views::exact::Intent,
    #[serde(default)]
    pub unit: crate::search_results::SearchUnit,
    pub query: String,
    #[serde(default = "default_generation")]
    pub generation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vector: Vec<f64>,
    pub provider: Option<Provider>,
    pub days: Option<u32>,
    #[serde(default)]
    pub exclude_self: bool,
    pub filter: Option<crate::search_filter::Filter>,
}

// Serde's skip predicate requires a borrowed field.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn related_discussion(intent: &crate::views::exact::Intent) -> bool {
    *intent == crate::views::exact::Intent::RelatedDiscussion
}

pub use mash_storage::views::candidates::{Candidates, Document};

pub(crate) fn default_generation() -> String {
    crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned()
}

pub(crate) fn valid_vector(vector: &[f64], dimensions: usize) -> bool {
    vector.len() == dimensions
        && vector.iter().all(|v| v.is_finite() && v.abs() <= 127.0)
        && vector
            .iter()
            .any(|v| v.abs() >= f64::from(f32::MIN_POSITIVE))
}

// Leave transport/decoding headroom inside the five-second client deadline.
// A cold vocabulary facet alone can take two seconds on retained node history.
pub(crate) const EXACT_SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
pub(crate) const EXACT_SEARCH_BUDGET_ERROR: &str = "exact query exceeded the shared search budget; narrow the query or metadata filters, or use match_mode=strict";
