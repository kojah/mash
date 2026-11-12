//! Identity of the single published views projection; query defaults are excluded.
use super::config::Build;
use crate::silver::certificate::Certificate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ViewsManifest")]
pub(crate) struct Manifest {
    pub(crate) schema_version: u32,
    pub(crate) build: Build,
    pub(crate) model_generation: String,
    pub(crate) silver: Certificate,
}

impl Manifest {
    pub(crate) fn new(build: Build, model_generation: String, silver: Certificate) -> Self {
        Self {
            schema_version: 1,
            build,
            model_generation,
            silver,
        }
    }
}
