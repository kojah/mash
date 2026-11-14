//! Identity of the single published views projection; query defaults are excluded.
use super::config::Build;
use crate::silver::certificate::Certificate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ViewsManifest")]
pub struct Manifest {
    pub schema_version: u32,
    pub build: Build,
    pub model_generation: String,
    pub silver: Certificate,
}

impl Manifest {
    pub fn new(build: Build, model_generation: String, silver: Certificate) -> Self {
        Self {
            schema_version: 1,
            build,
            model_generation,
            silver,
        }
    }
}
