//! Typed presentation of completed responses; retrieval and ranking are unchanged.
use super::models::{LocalArtifacts, ModelAvailability, StatusResponse};
use super::{Response, SearchMode, SearchResponse};
use crate::search_results::{ChatHit, ChatMatch, ChatMetadata, MessageReference, Results};
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

pub(crate) mod text;

pub(crate) fn serialize(
    response: &Response,
    detail: Detail,
) -> Result<serde_json::Value, serde_json::Error> {
    if matches!(detail, Detail::Extended) {
        return serde_json::to_value(response);
    }
    match response {
        Response::Search(value) => serde_json::to_value(Search::from(value)),
        Response::ListChats(value) => serde_json::to_value(List {
            chats: value
                .page
                .chats
                .iter()
                .map(|thread| Chat {
                    chat_id: &thread.chat_id,
                    provider: &thread.provider,
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

fn metadata(value: &ChatMetadata) -> ChatMetadata {
    let mut value = value.clone();
    value.enrichment = None;
    value
}

#[derive(Serialize)]
struct Message<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    lookup: Option<&'a crate::search_results::lookup::Lookup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lexical_match: Option<&'a crate::views::matching::Evidence>,
    id: &'a str,
    chat_id: &'a str,
    provider: &'a crate::provider::Provider,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    node_id: Option<&'a str>,
    role: &'a crate::role::Role,
    created_at: Option<i64>,
    content: String,
    content_truncated: bool,
    metadata: ChatMetadata,
    #[serde(skip_serializing_if = "<[MessageReference]>::is_empty")]
    copies: &'a [MessageReference],
}

#[derive(Serialize)]
struct Excerpt<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    lookup: Option<&'a crate::search_results::lookup::Lookup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lexical_match: Option<&'a crate::views::matching::Evidence>,
    message_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<&'a str>,
    node_id: Option<&'a str>,
    role: &'a crate::role::Role,
    created_at: Option<i64>,
    snippet: String,
    snippet_truncated: bool,
    #[serde(skip_serializing_if = "<[MessageReference]>::is_empty")]
    copies: &'a [MessageReference],
}

impl<'a> From<&'a ChatMatch> for Excerpt<'a> {
    fn from(value: &'a ChatMatch) -> Self {
        Self {
            lookup: value.lookup.as_ref(),
            lexical_match: value.lexical_match.as_ref(),
            message_id: &value.message_id,
            turn_id: value.turn_id.as_deref(),
            node_id: value.node_id.as_deref(),
            role: &value.role,
            created_at: value.created_at,
            // Results were masked under the output policy before they got here.
            snippet: crate::search_results::evidence_window_with(
                &value.snippet,
                "",
                EXCERPT_CHARS,
                crate::redaction::Policy::new(false),
            ),
            snippet_truncated: value.snippet.chars().count() > EXCERPT_CHARS
                || value.projection_span.start_char > 0
                || value.snippet.ends_with('…'),
            copies: &value.copies,
        }
    }
}

#[derive(Serialize)]
#[allow(
    clippy::struct_field_names,
    reason = "chat_id is the public identity field shared with search results"
)]
struct Chat<'a> {
    chat_id: &'a str,
    provider: &'a crate::provider::Provider,
    node_id: Option<&'a str>,
    #[serde(flatten)]
    metadata: ChatMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    matches: Vec<Excerpt<'a>>,
}

impl<'a> From<&'a ChatHit> for Chat<'a> {
    fn from(value: &'a ChatHit) -> Self {
        Self {
            chat_id: &value.chat_id,
            provider: &value.provider,
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
    Chats(Vec<Chat<'a>>),
    Messages(Vec<Message<'a>>),
}

#[derive(Serialize)]
struct Search<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    freshness_warnings: &'a Vec<String>,
    query: &'a str,
    intent: crate::views::exact::Intent,
    realized_mode: &'a SearchMode,
    unit: crate::search_results::SearchUnit,
    results: SearchResults<'a>,
    next_offset: Option<usize>,
    shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_context_error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a crate::views::diagnostics::Diagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<&'a str>,
}

impl<'a> From<&'a SearchResponse> for Search<'a> {
    fn from(value: &'a SearchResponse) -> Self {
        let results = match &value.results {
            Results::Chats(threads) => {
                SearchResults::Chats(threads.iter().map(Chat::from).collect())
            }
            Results::Messages(messages) => SearchResults::Messages(
                messages
                    .iter()
                    .map(|hit| Message {
                        lookup: hit.lookup.as_ref(),
                        lexical_match: hit.lexical_match.as_ref(),
                        id: &hit.id,
                        chat_id: &hit.chat_id,
                        provider: &hit.provider,
                        turn_id: hit.turn_id.as_deref(),
                        node_id: hit.node_id.as_deref(),
                        role: &hit.role,
                        created_at: hit.created_at,
                        content: crate::search_results::evidence_window_with(
                            &hit.content,
                            &value.query,
                            EXCERPT_CHARS,
                            crate::redaction::Policy::new(false),
                        ),
                        content_truncated: hit.content.chars().count() > EXCERPT_CHARS,
                        metadata: metadata(&hit.metadata),
                        copies: &hit.copies,
                    })
                    .collect(),
            ),
        };
        Self {
            freshness_warnings: &value.freshness_warnings,
            query: &value.query,
            intent: value.intent,
            realized_mode: &value.realized_mode,
            unit: value.unit,
            results,
            next_offset: value.next_offset,
            shared: value.shared,
            shared_error: value.shared_error.as_deref(),
            shared_context_error: value.shared_context_error.as_deref(),
            diagnostics: value.diagnostics.as_ref(),
            notice: value.notice.as_deref(),
        }
    }
}

#[derive(Serialize)]
struct List<'a> {
    chats: Vec<Chat<'a>>,
    next_offset: Option<usize>,
    shared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a super::chats::Diagnostics>,
}

#[derive(Serialize)]
struct Status<'a> {
    installation: &'a super::installation::Summary,
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<&'a super::coverage::Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shared_error: Option<&'a str>,
    local_artifacts: &'a LocalArtifacts,
    #[serde(skip_serializing_if = "Option::is_none")]
    silver: Option<&'a crate::silver::certificate::Certificate>,
    ready: bool,
    chats: u64,
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
            installation: &value.installation,
            coverage: value.coverage.as_ref(),
            shared_error: value.shared_error.as_deref(),
            local_artifacts: &value.local_artifacts,
            silver: value.silver.as_ref(),
            ready: value.ready,
            chats: value.chats,
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
