//! Built-in views: rebuildable retrieval artifacts over canonical silver history.
pub mod bulk;
pub mod config;
pub mod diagnostics;
pub use mash_storage::views::documents;
/// Exact-match literals and intents, with the ranking that needs fusion.
pub mod exact {
    pub use mash_storage::views::exact::*;

    pub fn rank(
        mut hits: Vec<crate::storage::SearchHit>,
        unit: crate::search_results::SearchUnit,
    ) -> Vec<crate::storage::SearchHit> {
        order(&mut hits);
        crate::semantic::fuse(hits, vec![], unit, super::config::Fusion::default())
    }
}
pub mod lexical_manifest;
pub mod local;
pub mod manifest;
pub use mash_storage::views::matching;
pub mod materialize;
pub mod qdrant;
pub mod readiness;
pub mod retrieval;
pub mod scope;
pub mod shared;
pub mod units;
pub mod vector;
