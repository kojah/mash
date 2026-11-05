//! Index scope shared by local views and the authenticated shared service.
use serde::Deserialize;

/// Selects a collection namespace, its source node, and embedding space.
/// Authentication belongs to the caller; the views engine needs only this scope.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scope {
    pub(crate) group_id: String,
    pub(crate) node_id: String,
    pub(crate) generation: String,
    pub(crate) dimensions: usize,
}
