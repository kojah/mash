//! Built-in views: rebuildable retrieval artifacts over canonical silver history.
pub(crate) mod bulk;
pub mod config;
pub(crate) mod diagnostics;
pub(crate) use mash_storage::views::documents;
/// Exact-match literals and intents, with the ranking that needs fusion.
pub mod exact {
    pub use mash_storage::views::exact::*;

    pub(crate) fn rank(
        mut hits: Vec<crate::storage::SearchHit>,
        unit: crate::search_results::SearchUnit,
    ) -> Vec<crate::storage::SearchHit> {
        order(&mut hits);
        crate::semantic::fuse(hits, vec![], unit, super::config::Fusion::default())
    }
}
pub(crate) mod lexical_manifest;
pub mod local;
pub(crate) mod manifest;
pub use mash_storage::views::matching;
pub(crate) mod materialize;
pub mod qdrant;
pub(crate) mod readiness;
pub(crate) mod retrieval;
pub(crate) mod scope;
pub(crate) mod shared;
pub(crate) mod units;
pub mod vector;
