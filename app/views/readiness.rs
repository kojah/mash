//! Artifact publication state, independent of query model availability.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct Artifacts {
    pub(crate) metadata: State,
    pub(crate) lexical: State,
    pub(crate) dense: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "ReadinessState")]
pub(crate) enum State {
    Missing,
    Stale,
    Ready,
    Disabled,
}

impl State {
    pub(crate) fn publication(present: bool, current: bool) -> Self {
        if !present {
            Self::Missing
        } else if current {
            Self::Ready
        } else {
            Self::Stale
        }
    }
}
