//! Artifact publication state, independent of query model availability.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Artifacts {
    pub metadata: State,
    pub lexical: State,
    pub dense: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "ReadinessState")]
pub enum State {
    Missing,
    Stale,
    Ready,
    Disabled,
}

impl State {
    pub fn publication(present: bool, current: bool) -> Self {
        if !present {
            Self::Missing
        } else if current {
            Self::Ready
        } else {
            Self::Stale
        }
    }
}
