//! Bounded literal scanning of immutable lexical documents, without vectors.
use super::{
    AppError, Identity, Store, Stored, api, collection, connector_hit, document_id, failure, tokens,
};
use crate::{
    shared::protocol,
    storage::SearchHit,
    views::exact::{self, Literal},
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
    "source_id",
    "object",
    "unit",
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

fn hit(
    point: api::Record<Stored<Document>>,
    active: &BTreeSet<String>,
) -> Result<SearchHit, AppError> {
    let d = match point.payload {
        Stored::Native(document) => document,
        Stored::Connector(document) => {
            // Exact search reads no vectors, so the generation is not checked here.
            let mut hit = connector_hit(&point.id, document, active, None)?;
            hit.lexical_score = Some(0.0);
            return Ok(hit);
        }
    };
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
        source: crate::source::Source::Provider(d.provider),
        object: crate::source::ObjectType::native_message(d.provider),
        ancestors: crate::source::native_ancestors(
            d.provider,
            &d.conversation.id,
            d.conversation.title.as_deref(),
        ),
        role: Some(m.role.clone()),
        author_kind: None,
        author: crate::source::author(Some(m.role), None),
        sequence: None,
        content: m.projection,
        copies: vec![],
        created_at: m.created_at,
        metadata: crate::search_results::ChatMetadata {
            enrichment: d.conversation.enrichment,
            title: d.conversation.title,
            project: d.project,
            repository: d.repository,
            created_at: d.conversation.created_at,
            updated_at: d.conversation.updated_at,
        },
        lexical_score: Some(0.0),
        lookup: None,
        lexical_match: None,
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
        let literal = Literal::with_mode(&input.query, input.match_mode)?;
        if let Some(filter) = &input.filter {
            filter.validate()?;
        }
        let active = active
            .iter()
            .filter(|node| !input.exclude_self || **node != identity.node_id)
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut filter = super::read::search_filter(&input, &active)?;
        let vocabulary = if input.match_mode == crate::views::matching::Mode::Strict
            || active.is_empty()
            || literal.mode() == crate::views::matching::Mode::Strict
        {
            Vec::new()
        } else {
            self.api
                .terms(&collection(&identity.group_id, false), &filter)
                .await?
        };
        let plan = crate::views::matching::Plan::new(&input.query, literal.mode(), &vocabulary)?;
        for group in plan.groups {
            filter.must.push(api::matches("tokens", group));
        }
        let mut offset = None;
        let mut scanned = 0;
        let mut hits = Vec::new();
        if !active.is_empty() {
            loop {
                let page = self
                    .api
                    .scroll_page::<Stored<Document>>(
                        &collection(&identity.group_id, false),
                        filter.clone(),
                        offset,
                        FIELDS,
                        100.min(MAX_CANDIDATES - scanned),
                    )
                    .await?;
                scanned += page.points.len();
                let mut page_hits = page
                    .points
                    .into_iter()
                    .filter(|point| match &point.payload {
                        Stored::Native(document) => !document.message.projection.is_empty(),
                        Stored::Connector(_) => true,
                    })
                    .map(|point| hit(point, &active))
                    .collect::<Result<Vec<_>, _>>()?;
                for hit in &mut page_hits {
                    crate::views::matching::annotate(hit, &literal, self.responses);
                }
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

#[cfg(test)]
mod budget_tests {
    use super::*;
    use axum::{Router, routing::post};

    #[tokio::test]
    async fn flexible_exact_search_allows_cold_vocabulary_and_keeps_a_deadline() {
        let identity = Identity {
            group_id: "test".into(),
            node_id: "node".into(),
            generation: "unused".into(),
            dimensions: 4,
        };
        let active = [identity.node_id.clone()].into_iter().collect();
        let name = collection(&identity.group_id, false);
        for (delay_ms, succeeds) in [(1800, true), (4200, false)] {
            let router = Router::new()
                .route(&format!("/collections/{name}/facet"), post(move || async move {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    r#"{"status":"ok","result":{"hits":[{"value":"bun","count":1},{"value":"mcp","count":1}]}}"#
                }))
                .route(&format!("/collections/{name}/points/scroll"), post(|| async {
                    r#"{"status":"ok","result":{"points":[],"next_page_offset":null}}"#
                }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let mut store = Store::unavailable();
            store.api = api::Client::new(&url, None, 64 * 1024).unwrap();
            let input = protocol::Search {
                match_mode: crate::views::matching::Mode::Flexible,
                vector_policy: None,
                intent: exact::Intent::ExactMention,
                unit: crate::search_results::SearchUnit::Chats,
                query: "bun mcp".into(),
                generation: String::new(),
                vector: Vec::new(),
                provider: None,
                days: None,
                exclude_self: false,
                filter: None,
            };
            let result = store.search(&identity, input, &active).await;
            if succeeds {
                assert!(result.unwrap().lexical.is_empty());
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.error.kind, crate::ErrorKind::Usage);
                assert_eq!(
                    error.error.message,
                    crate::shared::EXACT_SEARCH_BUDGET_ERROR
                );
            }
            server.abort();
        }
    }
}
