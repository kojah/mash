//! Bounded literal scanning of immutable lexical documents, without vectors.
use super::{AppError, Identity, Store, api, collection, document_id, failure, tokens};
use crate::{
    gold::exact::{self, Literal},
    shared::protocol,
    storage::SearchHit,
};
use serde::Deserialize;
use std::collections::BTreeSet;

const MAX_CANDIDATES: usize = 10_000;
const FIELDS: &[&str] = &[
    "node_id",
    "provider",
    "role",
    "conversation_id",
    "project",
    "repository",
    "created_at",
    "tokens",
    "conversation",
    "message.id",
    "message.conversation_id",
    "message.role",
    "message.projection",
    "message.created_at",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    id: String,
    conversation_id: String,
    role: crate::role::Role,
    projection: String,
    created_at: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    node_id: String,
    provider: crate::provider::Provider,
    role: crate::role::Role,
    conversation_id: String,
    project: Option<String>,
    repository: Option<String>,
    created_at: Option<i64>,
    tokens: Vec<String>,
    conversation: protocol::ConversationRecord,
    message: Message,
}

fn hit(point: api::Record<Document>, active: &BTreeSet<String>) -> Result<SearchHit, AppError> {
    let d = point.payload;
    let m = d.message;
    if point.id != document_id(&d.node_id, &m.id)
        || !active.contains(&d.node_id)
        || d.conversation_id != m.conversation_id
        || d.conversation.id != m.conversation_id
        || d.provider != d.conversation.provider
        || d.role != m.role
        || d.project != d.conversation.project
        || d.repository != d.conversation.repository
        || d.created_at != m.created_at
        || d.tokens != tokens(&m.projection)
        || m.projection.is_empty()
    {
        return Err(failure());
    }
    Ok(SearchHit {
        node_id: Some(d.node_id),
        id: m.id,
        conversation_id: m.conversation_id,
        provider: d.provider,
        role: m.role,
        content: m.projection,
        copies: vec![],
        created_at: m.created_at,
        metadata: crate::search_results::ThreadMetadata {
            enrichment: d.conversation.enrichment,
            title: d.conversation.title,
            project: d.project,
            repository: d.repository,
            created_at: d.conversation.created_at,
            updated_at: d.conversation.updated_at,
        },
        lexical_score: Some(0.0),
        semantic_score: None,
        fusion_score: 0.0,
        rerank_score: None,
    })
}

impl Store {
    pub(super) async fn search_exact(
        &self,
        identity: &Identity,
        input: protocol::Search,
        active: &BTreeSet<String>,
    ) -> Result<protocol::Candidates, AppError> {
        let literal = Literal::new(&input.query)?;
        if let Some(filter) = &input.filter {
            filter.validate()?;
        }
        let active = active
            .iter()
            .filter(|node| !input.exclude_self || **node != identity.node_id)
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut filter = super::read::search_filter(&input, &active)?;
        for word in tokens(&input.query).into_iter().collect::<BTreeSet<_>>() {
            filter.must.push(api::matches("tokens", vec![word]));
        }
        let mut offset = None;
        let mut scanned = 0;
        let mut hits = Vec::new();
        if !active.is_empty() {
            loop {
                let page = self
                    .api
                    .scroll_page::<Document>(
                        &collection(&identity.group_id, false),
                        filter.clone(),
                        offset,
                        FIELDS,
                        100.min(MAX_CANDIDATES - scanned),
                    )
                    .await?;
                scanned += page.points.len();
                let page_hits = page
                    .points
                    .into_iter()
                    .filter(|point| !point.payload.message.projection.is_empty())
                    .map(|point| hit(point, &active))
                    .collect::<Result<Vec<_>, _>>()?;
                hits.extend(
                    crate::search_results::eligible(page_hits)
                        .into_iter()
                        .filter(|hit| literal.matches(&hit.content)),
                );
                exact::order(&mut hits);
                hits.truncate(input.unit.candidate_limit());
                if page.next_page_offset.is_none() {
                    break;
                }
                if scanned == MAX_CANDIDATES {
                    return Err(AppError::usage(
                        "shared exact candidate budget reached (10000); narrow the query or metadata filters",
                    ));
                }
                offset = page.next_page_offset;
            }
        }
        Ok(protocol::Candidates {
            lexical: hits,
            semantic: vec![],
            documents: vec![],
        })
    }
}
