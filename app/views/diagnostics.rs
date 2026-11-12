//! Request-only policy and retrieval outcome; never canonical history.
use super::{
    config::{Fusion, Ranking},
    exact::Intent,
};
use crate::core::SearchResponse;
use crate::search_results::{Results, SearchUnit};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "DiagnosticsScope")]
pub(crate) enum Scope {
    Local,
    Shared,
    Combined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Source {
    Local,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Artifact {
    Metadata,
    Lexical,
    Dense,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub(crate) struct Diagnostics {
    pub(crate) policy_version: u16,
    pub(crate) intent: Intent,
    pub(crate) unit: SearchUnit,
    pub(crate) limit: usize,
    #[serde(default)]
    pub(crate) offset: usize,
    /// Selection is policy, not a claim about candidate counts or inference calls.
    pub(crate) selected_artifacts: Vec<Artifact>,
    /// Requested predicate names, not values, Boolean structure or removal counts.
    #[serde(default)]
    pub(crate) filter_fields: Vec<crate::search_filter::Field>,
    pub(crate) requested_scope: Scope,
    pub(crate) completed_sources: Vec<Source>,
    pub(crate) shared_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fusion: Option<Fusion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ranking: Option<Ranking>,
    /// Requested shared override, not a claim that remote retrieval completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) shared_vector_override: Option<super::vector::Search>,
    pub(crate) returned_results: usize,
    /// Core time includes configuration and model admission, excludes CLI startup.
    pub(crate) elapsed_ms: u64,
    #[serde(default)]
    pub(crate) stages: Vec<crate::observability::retrieval::Measurement>,
    #[serde(default)]
    pub(crate) shared_context_incomplete: bool,
}

impl Diagnostics {
    pub(crate) fn new(
        response: &SearchResponse,
        scope: Scope,
        page: crate::search_results::Page,
        fusion: Fusion,
        ranking: Ranking,
    ) -> Self {
        let hybrid = response.intent == Intent::RelatedDiscussion;
        let fallback = response.shared_error.is_some();
        Self {
            policy_version: 1,
            intent: response.intent,
            unit: response.unit,
            limit: page.limit,
            offset: page.offset,
            selected_artifacts: if hybrid {
                vec![Artifact::Metadata, Artifact::Lexical, Artifact::Dense]
            } else {
                vec![Artifact::Metadata, Artifact::Lexical]
            },
            filter_fields: Vec::new(),
            requested_scope: scope,
            completed_sources: match (scope, fallback) {
                (Scope::Combined, false) => vec![Source::Local, Source::Shared],
                (Scope::Shared, false) => vec![Source::Shared],
                _ => vec![Source::Local],
            },
            shared_fallback: fallback,
            fusion: hybrid.then_some(fusion),
            ranking: hybrid.then_some(ranking),
            shared_vector_override: None,
            returned_results: match &response.results {
                Results::Chats(hits) => hits.len(),
                Results::Messages(hits) => hits.len(),
            },
            elapsed_ms: 0,
            stages: Vec::new(),
            shared_context_incomplete: false,
        }
    }

    pub(crate) fn record(&mut self, stages: Vec<crate::observability::retrieval::Measurement>) {
        use crate::observability::retrieval::Stage;
        let local = stages.iter().any(|s| {
            matches!(s.stage, Stage::LocalExact | Stage::LocalCandidates) && s.completed_calls > 0
        });
        let shared = stages
            .iter()
            .any(|s| s.stage == Stage::SharedCandidates && s.completed_calls > 0);
        self.completed_sources = [
            local.then_some(Source::Local),
            shared.then_some(Source::Shared),
        ]
        .into_iter()
        .flatten()
        .collect();
        self.shared_fallback = self.requested_scope == Scope::Combined && !shared;
        self.shared_context_incomplete = stages
            .iter()
            .any(|s| s.stage == Stage::Context && s.completed_calls < s.calls);
        self.stages = stages;
    }
}
