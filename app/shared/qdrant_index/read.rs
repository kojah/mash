//! Group-scoped retrieval over the documents of nodes with compatible gold.
use super::{
    AppError, Document, Identity, Store, VectorIndex, api, collection, document_id, failure,
    sparse, tokens,
};
use crate::shared::protocol;
use crate::storage::SearchHit;
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::shared) fn validate_search(
    input: &protocol::Search,
    identity: &Identity,
) -> Result<(), AppError> {
    if let Some(policy) = input.vector_policy {
        policy.validate_request()?;
        if input.intent == crate::gold::exact::Intent::ExactMention {
            return Err(AppError::usage(
                "vector overrides require related-discussion intent",
            ));
        }
    }
    if input.intent == crate::gold::exact::Intent::ExactMention {
        return Ok(()); // Literal syntax and filters are validated by search_exact.
    }
    crate::search_query::validate(&input.query)?;
    if !crate::search_query::has_terms(&input.query)
        || input.query.len() > 16384
        || !protocol::valid_vector(&input.vector, identity.dimensions)
        || input.generation != identity.generation
    {
        return Err(AppError::usage("invalid search query or vector"));
    }
    if let Some(filter) = &input.filter {
        filter.validate()?;
    }
    Ok(())
}

fn metadata(c: &protocol::ConversationRecord) -> crate::search_results::ThreadMetadata {
    crate::search_results::ThreadMetadata {
        enrichment: c.enrichment.clone(),
        title: c.title.clone(),
        project: c.project.clone(),
        repository: c.repository.clone(),
        created_at: c.created_at,
        updated_at: c.updated_at,
    }
}
fn condition(input: &crate::search_filter::Filter) -> api::Condition {
    use crate::search_filter::Filter as F;
    let field = match input {
        F::All(children) => {
            return api::Condition::Nested(api::Filter {
                must: children.iter().map(condition).collect(),
                ..api::Filter::default()
            });
        }
        F::Any(children) => {
            return api::Condition::Nested(api::Filter {
                should: if children.is_empty() {
                    vec![api::impossible()]
                } else {
                    children.iter().map(condition).collect()
                },
                ..api::Filter::default()
            });
        }
        F::Not(child) => {
            return api::Condition::Nested(api::Filter {
                must_not: vec![condition(child)],
                ..api::Filter::default()
            });
        }
        F::After(time) => {
            return api::Condition::Range {
                key: "created_at".into(),
                range: api::Range {
                    gte: Some(time * 1000),
                    lt: None,
                },
            };
        }
        F::Before(time) => {
            return api::Condition::Range {
                key: "created_at".into(),
                range: api::Range {
                    gte: None,
                    lt: Some(time * 1000),
                },
            };
        }
        F::Node(values) => ("node_id", values.clone()),
        F::Provider(values) => ("provider", values.iter().map(ToString::to_string).collect()),
        F::Role(values) => (
            "role",
            values.iter().map(|r| r.as_str().to_owned()).collect(),
        ),
        F::Conversation(values) => ("conversation_id", values.clone()),
        F::Project(values) => ("project", values.clone()),
        F::Repository(values) => ("repository", values.clone()),
    };
    if field.1.is_empty() {
        api::impossible()
    } else {
        api::matches(field.0, field.1)
    }
}
pub(super) fn search_filter(
    input: &protocol::Search,
    active: &BTreeSet<String>,
) -> Result<api::Filter, AppError> {
    let mut filter = api::Filter {
        must: vec![api::matches("node_id", active.iter().cloned().collect())],
        ..api::Filter::default()
    };
    if let Some(provider) = input.provider {
        filter
            .must
            .push(api::matches("provider", vec![provider.to_string()]));
    }
    if let Some(days) = input.days {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AppError::usage("invalid system clock"))?
            .as_secs();
        let cutoff = i64::try_from(now)
            .unwrap_or(i64::MAX)
            .saturating_sub(i64::from(days) * 86400)
            .saturating_mul(1000);
        filter.must.push(api::Condition::Range {
            key: "created_at".into(),
            range: api::Range {
                gte: Some(cutoff),
                lt: None,
            },
        });
    }
    if let Some(expression) = &input.filter {
        filter.must.push(condition(expression));
    }
    Ok(filter)
}

impl Store {
    #[tracing::instrument(name = "backend_search", skip_all, fields(backend = "qdrant"))]
    pub(in crate::shared) async fn search(
        &self,
        identity: &Identity,
        input: protocol::Search,
        active: &BTreeSet<String>,
    ) -> Result<protocol::Candidates, AppError> {
        validate_search(&input, identity)?;
        if input.intent == crate::gold::exact::Intent::ExactMention {
            return tokio::time::timeout(std::time::Duration::from_millis(1500), self.search_exact(identity, input, active))
                .await.map_err(|_| AppError::usage("exact query exceeded the shared search budget; narrow the query or metadata filters"))?;
        }
        self.build.require_intent(input.intent)?;
        let configuration = self.active_configuration(identity).await?;
        let vector_policy = input.vector_policy.unwrap_or(configuration.vector_search);
        let active = active
            .iter()
            .filter(|node| !input.exclude_self || **node != identity.node_id)
            .cloned()
            .collect::<BTreeSet<_>>();
        if active.is_empty() {
            return Ok(protocol::Candidates {
                lexical: vec![],
                semantic: vec![],
                documents: vec![],
            });
        }
        let filter = search_filter(&input, &active)?;
        let mut lexical_filter = filter.clone();
        for word in tokens(&input.query) {
            lexical_filter.must.push(api::matches("tokens", vec![word]));
        }
        let (exact, ef) = match vector_policy {
            VectorIndex::Exact {} => (true, 200),
            VectorIndex::Hnsw { ef_runtime } => (false, ef_runtime),
        };
        let semantic_query = api::QueryRequest {
            query: api::QueryVector::Dense(input.vector),
            using: self.dense_name(identity).into(),
            filter,
            params: api::Params { exact, hnsw_ef: ef },
            limit: input.unit.candidate_limit(),
            with_payload: true,
        };
        let lexical_query = api::QueryRequest {
            query: api::QueryVector::Sparse(sparse(&input.query, true)),
            using: "lexical".into(),
            filter: lexical_filter,
            params: api::Params {
                exact: true,
                hnsw_ef: ef,
            },
            limit: input.unit.candidate_limit(),
            with_payload: true,
        };
        let name = collection(&identity.group_id, false);
        let (lexical, semantic) = tokio::try_join!(
            self.api.query::<Document>(&name, &lexical_query),
            self.api.query::<Document>(&name, &semantic_query)
        )?;
        let lexical = hits(lexical, true, identity, &active, &self.build)?;
        let semantic = hits(semantic, false, identity, &active, &self.build)?;
        let documents: BTreeMap<_, _> = lexical
            .iter()
            .chain(&semantic)
            .map(|h| {
                (
                    (h.node_id.clone().unwrap_or_default(), h.id.clone()),
                    h.content.clone(),
                )
            })
            .collect();
        Ok(protocol::Candidates {
            lexical,
            semantic,
            documents: documents
                .into_iter()
                .map(|((node_id, id), projection)| protocol::Document {
                    node_id,
                    id,
                    projection,
                })
                .collect(),
        })
    }
}
fn hits(
    points: Vec<api::Scored<Document>>,
    lexical: bool,
    identity: &Identity,
    active: &BTreeSet<String>,
    build: &crate::gold::config::Build,
) -> Result<Vec<SearchHit>, AppError> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for point in points {
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
            || if build.includes_dense(&m.role, &m.projection) {
                m.generation.as_deref() != Some(&identity.generation)
                    || m.vector
                        .as_ref()
                        .is_none_or(|v| !protocol::valid_vector(v, identity.dimensions))
            } else {
                !lexical || m.vector.is_some() || m.generation.is_some()
            }
            || !seen.insert((d.node_id.clone(), m.id.clone()))
        {
            return Err(failure());
        }
        output.push(SearchHit {
            node_id: Some(d.node_id),
            id: m.id,
            conversation_id: m.conversation_id,
            provider: d.provider,
            role: m.role,
            content: m.projection,
            copies: vec![],
            created_at: m.created_at,
            metadata: metadata(&d.conversation),
            lexical_score: lexical.then_some(-f64::from(point.score)),
            semantic_score: (!lexical).then_some(point.score),
            fusion_score: 0.0,
            rerank_score: None,
        });
    }
    Ok(output)
}
