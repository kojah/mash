//! Group-scoped retrieval. Check active versions again before returning any result.
use super::CatalogRecord;
use super::{
    AppError, Document, Identity, NodeHead, PointKind, Store, VectorIndex, api, collection,
    failure, point_id, sparse, tokens,
};
use crate::shared::{Freshness, NodeFreshness, protocol, validation::valid_id};
use crate::storage::{Counts, Message, SearchHit};
use std::collections::{BTreeMap, BTreeSet};

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
fn search_filter(
    input: &protocol::Search,
    active: &BTreeMap<&String, &NodeHead>,
) -> Result<api::Filter, AppError> {
    let mut filter = api::Filter {
        must: vec![api::matches(
            "version",
            active.values().map(|h| h.version.clone()).collect(),
        )],
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
    async fn heads(&self, identity: &Identity) -> Result<BTreeMap<String, NodeHead>, AppError> {
        let points = self
            .api
            .scroll::<NodeHead>(
                &collection(&identity.group_id, true),
                api::Filter {
                    must: vec![api::matches("kind", vec!["head".into()])],
                    ..api::Filter::default()
                },
            )
            .await?;
        let mut heads = BTreeMap::new();
        for point in points {
            let h = point.payload;
            if h.kind != PointKind::Head
                || point.id != point_id(&["head", &h.node_id])
                || h.head.as_ref().is_none_or(|head| head.version != h.version)
                || h.manifest.is_none()
                || heads.contains_key(&h.node_id)
            {
                return Err(failure());
            }
            heads.insert(h.node_id.clone(), h);
        }
        Ok(heads)
    }
    async fn stable_heads(
        &self,
        identity: &Identity,
        heads: &BTreeMap<String, NodeHead>,
    ) -> Result<(), AppError> {
        let current = self.heads(identity).await?;
        if !heads
            .iter()
            .map(|(n, h)| (n, &h.version))
            .eq(current.iter().map(|(n, h)| (n, &h.version)))
        {
            return Err(crate::shared::validation::conflict(
                "Qdrant group changed while reading; retry the operation",
            ));
        }
        Ok(())
    }
    pub(in crate::shared) async fn status(&self, identity: &Identity) -> Result<Counts, AppError> {
        Ok(self
            .node(identity)
            .await?
            .head
            .map_or_else(Counts::default, |h| h.counts))
    }
    pub(in crate::shared) async fn freshness(
        &self,
        identity: &Identity,
    ) -> Result<Freshness, AppError> {
        let heads = self.heads(identity).await?;
        let mut nodes = Vec::new();
        for (node_id, node) in &heads {
            let h = node.head.as_ref().ok_or_else(failure)?;
            nodes.push(NodeFreshness {
                node_id: node_id.clone(),
                revision: h.revision,
                last_synced_at: h.last_synced_at.clone(),
                last_updated_at: h.last_updated_at.clone(),
                conversations: i64::try_from(h.counts.conversations).unwrap_or(i64::MAX),
                messages: i64::try_from(h.counts.messages).unwrap_or(i64::MAX),
            });
        }
        self.stable_heads(identity, &heads).await?;
        Ok(Freshness {
            group_id: identity.group_id.clone(),
            nodes,
        })
    }
    pub(in crate::shared) async fn state(
        &self,
        identity: &Identity,
    ) -> Result<protocol::NodeState, AppError> {
        let loaded = self.load(identity).await?;
        let mut state = loaded.data.state(identity);
        if let Some(h) = loaded.head {
            state.instance = Some(h.instance);
            state.revision = h.revision;
            state.source_silver = h.source_silver;
            state.silver_scans = h.silver_scans;
            if h.generation != identity.generation {
                for source in &mut state.sources {
                    source.ready = false;
                }
            }
        }
        Ok(state)
    }
    #[tracing::instrument(name = "backend_list_threads", skip_all, fields(backend = "qdrant"))]
    pub(in crate::shared) async fn list_threads(
        &self,
        identity: &Identity,
        input: crate::thread_catalog::Request,
    ) -> Result<crate::thread_catalog::Page, AppError> {
        let mut collector = crate::thread_catalog::Collector::new(input.clone())?;
        let heads = self.heads(identity).await?;
        for node in heads
            .keys()
            .filter(|n| input.admits_node(n, &identity.node_id))
        {
            let head = heads.get(node).ok_or_else(failure)?;
            let points = self
                .api
                .scroll::<CatalogRecord>(
                    &collection(&identity.group_id, true),
                    api::Filter {
                        must: vec![
                            api::matches("kind", vec!["catalog".into()]),
                            api::matches("version", vec![head.version.clone()]),
                        ],
                        ..api::Filter::default()
                    },
                )
                .await?;
            if u64::try_from(points.len()).unwrap_or(u64::MAX)
                != head.head.as_ref().ok_or_else(failure)?.counts.conversations
            {
                return Err(failure());
            }
            for point in points {
                let payload = point.payload;
                let row = &payload.conversation;
                if payload.kind != PointKind::Catalog
                    || payload.version != head.version
                    || payload.node_id != *node
                    || point.id != point_id(&["catalog", &head.version, &row.id])
                {
                    return Err(failure());
                }
                collector.push(crate::thread_catalog::Thread {
                    conversation_id: row.id.clone(),
                    provider: row.provider,
                    node_id: node.clone(),
                    metadata: metadata(row),
                });
            }
        }
        self.stable_heads(identity, &heads).await?;
        Ok(collector.finish())
    }
    #[tracing::instrument(name = "backend_search", skip_all, fields(backend = "qdrant"))]
    pub(in crate::shared) async fn search(
        &self,
        identity: &Identity,
        input: protocol::Search,
    ) -> Result<protocol::Candidates, AppError> {
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
        let heads = self.heads(identity).await?;
        let active: BTreeMap<_, _> = heads
            .iter()
            .filter(|(n, _)| !input.exclude_self || **n != identity.node_id)
            .collect();
        if active.values().any(|node| {
            node.head.as_ref().is_none_or(|h| {
                h.generation != identity.generation
                    || h.counts.embeddings != h.counts.searchable_messages
            })
        }) {
            return Err(AppError::shared(
                "shared embeddings are incomplete or incompatible; run mash index on publishing nodes",
            ));
        }
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
        let (exact, ef) = match self.vector_index {
            VectorIndex::Exact {} => (true, 200),
            VectorIndex::Hnsw { ef_runtime } => (false, ef_runtime),
        };
        let semantic_query = api::QueryRequest {
            query: api::QueryVector::Dense(input.vector),
            using: "dense",
            filter,
            params: api::Params { exact, hnsw_ef: ef },
            limit: input.unit.candidate_limit(),
            with_payload: true,
        };
        let lexical_query = api::QueryRequest {
            query: api::QueryVector::Sparse(sparse(&input.query, true)),
            using: "lexical",
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
        let lexical = hits(lexical, true, identity, &active)?;
        let semantic = hits(semantic, false, identity, &active)?;
        self.stable_heads(identity, &heads).await?;
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
    #[tracing::instrument(name = "backend_view", skip_all, fields(backend = "qdrant"))]
    pub(in crate::shared) async fn view(
        &self,
        identity: &Identity,
        input: protocol::View,
    ) -> Result<Vec<Message>, AppError> {
        valid_id(&input.id)?;
        valid_id(&input.node_id)?;
        if input.context > 1000 {
            return Err(AppError::usage("invalid view context"));
        }
        let other = Identity {
            node_id: input.node_id.clone(),
            ..identity.clone()
        };
        let head = self.node(&other).await?;
        if head.head.is_none() {
            return Ok(vec![]);
        }
        let name = collection(&identity.group_id, false);
        let records = self
            .api
            .retrieve::<Document>(&name, &[point_id(&["message", &head.version, &input.id])])
            .await?;
        let Some(anchor) = super::context::anchor(records, &head, &input)? else {
            self.unchanged(&other, &head.version).await?;
            return Ok(vec![]);
        };
        let records = self
            .api
            .scroll_fields::<super::context::Outline>(
                &name,
                api::Filter {
                    must: vec![
                        api::matches("version", vec![head.version.clone()]),
                        api::matches("node_id", vec![input.node_id.clone()]),
                        api::matches("conversation_id", vec![anchor.conversation_id.clone()]),
                    ],
                    ..api::Filter::default()
                },
                super::context::FIELDS,
            )
            .await?;
        let (selection, head, input) = crate::blocking(move || {
            super::context::select(&anchor, records, &head, &input)
                .map(|selection| (selection, head, input))
        })
        .await?;
        let mut records = Vec::new();
        for ids in selection.ids(&head.version).chunks(100) {
            records.extend(self.api.retrieve::<Document>(&name, ids).await?);
        }
        self.unchanged(&other, &head.version).await?;
        crate::blocking(move || selection.messages(records, &head, &input)).await
    }
}
fn hits(
    points: Vec<api::Scored<Document>>,
    lexical: bool,
    identity: &Identity,
    active: &BTreeMap<&String, &NodeHead>,
) -> Result<Vec<SearchHit>, AppError> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for point in points {
        let d = point.payload;
        let m = d.message;
        if point.id != point_id(&["message", &d.version, &m.id])
            || active
                .get(&d.node_id)
                .is_none_or(|h| h.version != d.version)
            || d.conversation_id != m.conversation_id
            || d.conversation.id != m.conversation_id
            || d.provider != d.conversation.provider
            || d.role != m.role
            || d.project != d.conversation.project
            || d.repository != d.conversation.repository
            || d.created_at != m.created_at
            || d.tokens != tokens(&m.projection)
            || m.projection.is_empty()
            || m.generation.as_deref() != Some(&identity.generation)
            || m.vector
                .as_ref()
                .is_none_or(|v| !protocol::valid_vector(v, identity.dimensions))
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
