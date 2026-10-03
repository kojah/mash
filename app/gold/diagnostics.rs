//! Request-only policy and retrieval outcome; never canonical history.
use super::{config::Fusion, exact::Intent};
use crate::core::SearchResponse;
use crate::search_results::{Results, SearchUnit};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Scope {
    Local,
    Shared,
    Combined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Source {
    Local,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Artifact {
    Metadata,
    Lexical,
    Dense,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Diagnostics {
    pub(crate) policy_version: u16,
    pub(crate) intent: Intent,
    pub(crate) unit: SearchUnit,
    pub(crate) limit: usize,
    /// Selection is policy, not a claim about candidate counts or inference calls.
    pub(crate) selected_artifacts: Vec<Artifact>,
    pub(crate) requested_scope: Scope,
    pub(crate) completed_sources: Vec<Source>,
    pub(crate) shared_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fusion: Option<Fusion>,
    pub(crate) returned_results: usize,
    /// Core time includes configuration and model admission, excludes CLI startup.
    pub(crate) elapsed_ms: u64,
}

impl Diagnostics {
    pub(crate) fn new(
        response: &SearchResponse,
        scope: Scope,
        limit: usize,
        fusion: Fusion,
    ) -> Self {
        let hybrid = response.intent == Intent::RelatedDiscussion;
        let fallback = response.shared_error.is_some();
        Self {
            policy_version: 1,
            intent: response.intent,
            unit: response.unit,
            limit,
            selected_artifacts: if hybrid {
                vec![Artifact::Metadata, Artifact::Lexical, Artifact::Dense]
            } else {
                vec![Artifact::Metadata, Artifact::Lexical]
            },
            requested_scope: scope,
            completed_sources: match (scope, fallback) {
                (Scope::Combined, false) => vec![Source::Local, Source::Shared],
                (Scope::Shared, false) => vec![Source::Shared],
                _ => vec![Source::Local],
            },
            shared_fallback: fallback,
            fusion: hybrid.then_some(fusion),
            returned_results: match &response.results {
                Results::Threads(hits) => hits.len(),
                Results::Messages(hits) => hits.len(),
            },
            elapsed_ms: 0,
        }
    }
}
