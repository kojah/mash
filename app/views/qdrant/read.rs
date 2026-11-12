//! Group-scoped retrieval over the documents of nodes with compatible views.
use super::{
    AppError, Document, Scope, Store, VectorIndex, api, collection, document_hit, failure, sparse,
};
use crate::storage::SearchHit;
use crate::views::retrieval as protocol;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn validate_search(input: &protocol::Search, identity: &Scope) -> Result<(), AppError> {
    if let Some(policy) = input.vector_policy {
        policy.validate_request()?;
        if input.intent == crate::views::exact::Intent::ExactMention {
            return Err(AppError::usage(
                "vector overrides require related-discussion intent",
            ));
        }
    }
    if input.intent == crate::views::exact::Intent::ExactMention {
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
        F::Chat(values) => ("chat_id", values.clone()),
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
    /// Preserve original lexical candidates before considering expanded terms.
    async fn lexical(
        &self,
        identity: &Scope,
        input: &protocol::Search,
        active: &BTreeSet<String>,
        filter: &api::Filter,
        ef: usize,
    ) -> Result<Vec<SearchHit>, AppError> {
        let name = collection(&identity.group_id, false);
        let original = crate::views::matching::Plan::new(
            &input.query,
            crate::views::matching::Mode::Strict,
            &[],
        )?;
        let request = |plan: &crate::views::matching::Plan| {
            let mut scoped = filter.clone();
            for group in &plan.groups {
                scoped.must.push(api::matches("tokens", group.clone()));
            }
            api::QueryRequest {
                query: api::QueryVector::Sparse(sparse(&plan.sparse_text(), true)),
                using: "lexical".into(),
                filter: scoped,
                params: api::Params {
                    exact: true,
                    hnsw_ef: ef,
                },
                limit: input.unit.candidate_limit(),
                with_payload: true,
            }
        };
        let original_request = request(&original);
        let strict = self.api.query::<Document>(&name, &original_request);
        let expand = async {
            if input.match_mode == crate::views::matching::Mode::Strict
                || (input.query.trim().starts_with('"') && input.query.trim().ends_with('"'))
            {
                return Ok((original, Vec::new()));
            }
            let vocabulary = self.vocabulary(identity, active, filter).await?;
            let plan =
                crate::views::matching::Plan::new(&input.query, input.match_mode, &vocabulary)?;
            let points = self.api.query::<Document>(&name, &request(&plan)).await?;
            Ok::<_, AppError>((plan, points))
        };
        let (points, (plan, expanded)) = tokio::try_join!(strict, expand)?;
        let mut lexical = hits(points, true, identity, active, &self.build)?;
        let mut seen = lexical
            .iter()
            .map(|h| (h.node_id.clone(), h.id.clone()))
            .collect::<BTreeSet<_>>();
        for mut hit in hits(expanded, true, identity, active, &self.build)? {
            if seen.insert((hit.node_id.clone(), hit.id.clone())) {
                hit.lexical_match =
                    plan.evidence(&self.responses.text(&hit.content), input.match_mode);
                lexical.push(hit);
            }
        }
        lexical.truncate(input.unit.candidate_limit());
        Ok(lexical)
    }
    #[tracing::instrument(name = "backend_search", skip_all, fields(backend = "qdrant"))]
    pub(crate) async fn search(
        &self,
        identity: &Scope,
        mut input: protocol::Search,
        active: &BTreeSet<String>,
    ) -> Result<protocol::Candidates, AppError> {
        validate_search(&input, identity)?;
        if input.intent == crate::views::exact::Intent::ExactMention {
            return tokio::time::timeout(
                crate::views::retrieval::EXACT_SEARCH_TIMEOUT,
                self.search_exact(identity, input, active),
            )
            .await
            .map_err(|_| AppError::usage(crate::views::retrieval::EXACT_SEARCH_BUDGET_ERROR))?;
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
        let (exact, ef) = match vector_policy {
            VectorIndex::Exact {} => (true, 200),
            VectorIndex::Hnsw { ef_runtime } => (false, ef_runtime),
        };
        let semantic_query = api::QueryRequest {
            query: api::QueryVector::Dense(std::mem::take(&mut input.vector)),
            using: self.dense_name(identity).into(),
            filter: filter.clone(),
            params: api::Params { exact, hnsw_ef: ef },
            limit: input.unit.candidate_limit(),
            with_payload: true,
        };
        let name = collection(&identity.group_id, false);
        let lexical_query = self.lexical(identity, &input, &active, &filter, ef);
        let (lexical, semantic) = tokio::try_join!(
            lexical_query,
            self.api.query::<Document>(&name, &semantic_query)
        )?;
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
    identity: &Scope,
    active: &BTreeSet<String>,
    build: &crate::views::config::Build,
) -> Result<Vec<SearchHit>, AppError> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for point in points {
        let mut hit = document_hit(
            &point.id,
            point.payload,
            active,
            Some((identity, build, lexical)),
        )?;
        if !seen.insert((hit.node_id.clone(), hit.id.clone())) {
            return Err(failure());
        }
        hit.lexical_score = lexical.then_some(-f64::from(point.score));
        hit.semantic_score = (!lexical).then_some(point.score);
        output.push(hit);
    }
    Ok(output)
}
