//! Request-only policy and retrieval outcome; never canonical history.
use super::{
    config::{Fusion, Ranking},
    exact::Intent,
};
use crate::search_results::{Results, SearchUnit};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[schemars(rename = "DiagnosticsScope")]
pub enum Scope {
    Local,
    Shared,
    Combined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Local,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Artifact {
    Metadata,
    Lexical,
    Dense,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Diagnostics {
    pub policy_version: u16,
    pub intent: Intent,
    pub unit: SearchUnit,
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    /// Selection is policy, not a claim about candidate counts or inference calls.
    pub selected_artifacts: Vec<Artifact>,
    /// Requested predicate names, not values, Boolean structure or removal counts.
    #[serde(default)]
    pub filter_fields: Vec<crate::search_filter::Field>,
    pub requested_scope: Scope,
    pub completed_sources: Vec<Source>,
    pub shared_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fusion: Option<Fusion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranking: Option<Ranking>,
    /// Requested shared override, not a claim that remote retrieval completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_vector_override: Option<super::vector::Search>,
    pub returned_results: usize,
    /// Core time includes configuration and model admission, excludes CLI startup.
    pub elapsed_ms: u64,
    #[serde(default)]
    pub stages: Vec<crate::observability::retrieval::Measurement>,
    #[serde(default)]
    pub shared_context_incomplete: bool,
}

impl Diagnostics {
    pub fn new(
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

    pub fn record(&mut self, stages: Vec<crate::observability::retrieval::Measurement>) {
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

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchMode {
    Lexical,
    LocalLexical,
    MergedLexical,
    Hybrid,
    LocalHybrid,
    MergedHybrid,
    Unavailable,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SearchResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub freshness_warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Diagnostics>,
    pub query: String,
    #[serde(default)]
    pub intent: Intent,
    pub realized_mode: SearchMode,
    pub unit: crate::search_results::SearchUnit,
    pub results: crate::search_results::Results,
    /// Offset of the next page of the same ranked list; null on the last page.
    #[serde(default)]
    pub next_offset: Option<usize>,
    pub shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_context_error: Option<String>,
    /// Why an empty result may not mean the history lacks the subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}
