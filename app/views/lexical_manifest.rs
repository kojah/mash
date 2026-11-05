//! Local lexical publication, independent of dense recipes and model assets.
use crate::silver::certificate::Certificate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) schema_version: u32,
    /// Qdrant lexical payloads and sparse retrieval, with literal verification.
    pub(crate) representation_version: u32,
    pub(crate) silver: Certificate,
    pub(crate) searchable_messages: u64,
}

impl Manifest {
    pub(crate) fn new(silver: Certificate, searchable_messages: u64) -> Self {
        Self {
            schema_version: 1,
            representation_version: 2,
            silver,
            searchable_messages,
        }
    }
}
