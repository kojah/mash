//! Typed presentation of completed responses; retrieval and ranking are unchanged.
use super::models::{LocalArtifacts, ModelAvailability, StatusResponse};
use super::{Response, SearchMode, SearchResponse};
use crate::search_results::{MessageReference, Results, ThreadHit, ThreadMatch, ThreadMetadata};
use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Detail {
    #[default]
    Compact,
    Extended,
}

const EXCERPT_CHARS: usize = 600;

pub(crate) fn serialize(
    response: &Response,
    detail: Detail,
) -> Result<serde_json::Value, serde_json::Error> {
    if matches!(detail, Detail::Extended) {
        return serde_json::to_value(response);
    }
    match response {
        Response::Search(value) => serde_json::to_value(Search::from(value)),
        Response::ListThreads(value) => serde_json::to_value(List {
            threads: value
                .page
                .threads
                .iter()
                .map(|thread| Thread {
                    conversation_id: &thread.conversation_id,
                    provider: thread.provider,
                    node_id: Some(&thread.node_id),
                    metadata: metadata(&thread.metadata),
                    score: None,
                    matches: Vec::new(),
                })
                .collect(),
            next_offset: value.page.next_offset,
            shared: value.shared,
            shared_error: value.shared_error.as_deref(),
            diagnostics: value.diagnostics.as_ref(),
        }),
        Response::Status(value) => serde_json::to_value(Status::from(value.as_ref())),
        _ => serde_json::to_value(response),
    }
}

fn metadata(value: &ThreadMetadata) -> ThreadMetadata {
    let mut value = value.clone();
    value.enrichment = None;
    value
}

#[derive(Serialize)]
struct Message<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    lexical_match: Option<&'a crate::gold::matching::Evidence>,
    id: &'a str,
    conversation_id: &'a str,
    provider: crate::provider::Provider,
    node_id: Option<&'a str>,
    role: &'a crate::role::Role,
    created_at: Option<i64>,
    content: String,
    content_truncated: bool,
    metadata: ThreadMetadata,
    #[serde(skip_serializing_if = "<[MessageReference]>::is_empty")]
    copies: &'a [MessageReference],
}

#[derive(Serialize)]
struct Excerpt<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    lexical_match: Option<&'a crate::gold::matching::Evidence>,
    message_id: &'a str,
    node_id: Option<&'a str>,
    role: &'a crate::role::Role,
    created_at: Option<i64>,
    snippet: String,
    snippet_truncated: bool,
    #[serde(skip_serializing_if = "<[MessageReference]>::is_empty")]
    copies: &'a [MessageReference],
}

impl<'a> From<&'a ThreadMatch> for Excerpt<'a> {
    fn from(value: &'a ThreadMatch) -> Self {
        Self {
            lexical_match: value.lexical_match.as_ref(),
            message_id: &value.message_id,
            node_id: value.node_id.as_deref(),
            role: &value.role,
            created_at: value.created_at,
            snippet: crate::search_results::evidence_window(&value.snippet, "", EXCERPT_CHARS),
            snippet_truncated: value.snippet.chars().count() > EXCERPT_CHARS
                || value.projection_span.start_char > 0
                || value.snippet.ends_with('…'),
            copies: &value.copies,
        }
    }
}

#[derive(Serialize)]
struct Thread<'a> {
    conversation_id: &'a str,
    provider: crate::provider::Provider,
    node_id: Option<&'a str>,
    #[serde(flatten)]
    metadata: ThreadMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    matches: Vec<Excerpt<'a>>,
}

impl<'a> From<&'a ThreadHit> for Thread<'a> {
    fn from(value: &'a ThreadHit) -> Self {
        Self {
            conversation_id: &value.conversation_id,
            provider: value.provider,
            node_id: value.node_id.as_deref(),
            metadata: metadata(&value.metadata),
            score: Some(value.score),
            matches: value.matches.iter().map(Excerpt::from).collect(),
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum SearchResults<'a> {
    Threads(Vec<Thread<'a>>),
    Messages(Vec<Message<'a>>),
}

#[derive(Serialize)]
struct Search<'a> {
    query: &'a str,
    intent: crate::gold::exact::Intent,
    realized_mode: &'a SearchMode,
    unit: crate::search_results::SearchUnit,
    results: SearchResults<'a>,
    shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_context_error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a crate::gold::diagnostics::Diagnostics>,
}

impl<'a> From<&'a SearchResponse> for Search<'a> {
    fn from(value: &'a SearchResponse) -> Self {
        let results = match &value.results {
            Results::Threads(threads) => {
                SearchResults::Threads(threads.iter().map(Thread::from).collect())
            }
            Results::Messages(messages) => SearchResults::Messages(
                messages
                    .iter()
                    .map(|hit| Message {
                        lexical_match: hit.lexical_match.as_ref(),
                        id: &hit.id,
                        conversation_id: &hit.conversation_id,
                        provider: hit.provider,
                        node_id: hit.node_id.as_deref(),
                        role: &hit.role,
                        created_at: hit.created_at,
                        content: crate::search_results::evidence_window(
                            &hit.content,
                            &value.query,
                            EXCERPT_CHARS,
                        ),
                        content_truncated: hit.content.chars().count() > EXCERPT_CHARS,
                        metadata: metadata(&hit.metadata),
                        copies: &hit.copies,
                    })
                    .collect(),
            ),
        };
        Self {
            query: &value.query,
            intent: value.intent,
            realized_mode: &value.realized_mode,
            unit: value.unit,
            results,
            shared: value.shared,
            shared_error: value.shared_error.as_deref(),
            shared_context_error: value.shared_context_error.as_deref(),
            diagnostics: value.diagnostics.as_ref(),
        }
    }
}

#[derive(Serialize)]
struct List<'a> {
    threads: Vec<Thread<'a>>,
    next_offset: Option<usize>,
    shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a super::threads::Diagnostics>,
}

#[derive(Serialize)]
struct Status<'a> {
    local_artifacts: &'a LocalArtifacts,
    #[serde(skip_serializing_if = "Option::is_none")]
    silver: Option<&'a crate::silver::certificate::Certificate>,
    ready: bool,
    conversations: u64,
    messages: u64,
    searchable_messages: u64,
    embeddings: u64,
    models_installed: bool,
    query_models: &'a ModelAvailability,
    semantic_support: bool,
    realized_mode: &'a SearchMode,
    recommended_action: &'a Option<crate::Action>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared: Option<crate::shared::PublicationSummary<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_index: Option<crate::shared::FreshnessSummary<'a>>,
}

impl<'a> From<&'a StatusResponse> for Status<'a> {
    fn from(value: &'a StatusResponse) -> Self {
        Self {
            local_artifacts: &value.local_artifacts,
            silver: value.silver.as_ref(),
            ready: value.ready,
            conversations: value.conversations,
            messages: value.messages,
            searchable_messages: value.searchable_messages,
            embeddings: value.embeddings,
            models_installed: value.models_installed,
            query_models: &value.query_models,
            semantic_support: value.semantic_support,
            realized_mode: &value.realized_mode,
            recommended_action: &value.recommended_action,
            shared: value
                .shared
                .as_ref()
                .map(crate::shared::Publication::summary),
            shared_index: value
                .shared_index
                .as_ref()
                .map(crate::shared::Freshness::summary),
        }
    }
}

#[cfg(test)]
mod tests;
