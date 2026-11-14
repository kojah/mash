//! Local lexical publication, independent of dense recipes and model assets.
use crate::silver::certificate::Certificate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "LexicalManifest")]
pub struct Manifest {
    pub schema_version: u32,
    /// Qdrant lexical payloads and their ranked text index, with literal verification.
    pub representation_version: u32,
    pub silver: Certificate,
    pub searchable_messages: u64,
}

impl Manifest {
    pub fn new(silver: Certificate, searchable_messages: u64) -> Self {
        Self {
            schema_version: 1,
            representation_version: 2,
            silver,
            searchable_messages,
        }
    }
}
