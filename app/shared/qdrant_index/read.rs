//! Group-scoped retrieval. Check active versions again before returning any result.
use super::CatalogRecord;
use super::{
    AppError, Document, Identity, NodeHead, PointKind, Store, VectorIndex, api, collection,
    failure, point_id, sparse, tokens,
};
use crate::gold::readiness::{Artifacts, State};
use crate::records::{Counts, Message, SearchHit};
use crate::shared::{Freshness, NodeFreshness, protocol, validation::valid_id};
use std::collections::{BTreeMap, BTreeSet};

fn validate_search(input: &protocol::Search, identity: &Identity) -> Result<(), AppError> {
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
fn check_recipe(
    heads: &BTreeMap<String, NodeHead>,
    identity: &Identity,
    build: &crate::gold::config::Build,
) -> Result<(), AppError> {
    // A node filter must not hide a mixed group recipe during replacement.
    if heads.values().any(|node| {
        node.head.as_ref().is_none_or(|h| {
            h.gold_manifest
                .as_ref()
                .is_none_or(|m| !m.matches(build, &identity.generation, h.silver_revision.as_ref()))
        })
    }) {
        return Err(AppError::shared(
            "shared gold recipe is stale or unverified; run mash index --rebuild-shared-gold on publishing nodes",
        ));
    }
    Ok(())
}

pub(super) fn search_filter(
    input: &protocol::Search,
    active: &BTreeMap<&String, &NodeHead>,
) -> Result<api::Filter, AppError> {
    let mut filter = api::Filter {
        must: vec![api::matches(
            "version",
            active.values().flat_map(|h| h.versions()).collect(),
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
    pub(in crate::shared) async fn capabilities(
        &self,
        identity: &Identity,
    ) -> Result<crate::gold::config::Build, AppError> {
        Ok(self.active_configuration(identity).await?.build)
    }
    pub(super) async fn heads(
        &self,
        identity: &Identity,
    ) -> Result<BTreeMap<String, NodeHead>, AppError> {
        super::configuration::catalogue_nodes(self.active_configuration(identity).await?)
    }
    pub(super) async fn stable_heads(
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
        let group_gold = self.gold_configuration(identity).await?;
        let heads = self.heads(identity).await?;
        let mut nodes = Vec::new();
        for (node_id, node) in &heads {
            let h = node.head.as_ref().ok_or_else(failure)?;
            if let Some(revision) = &h.silver_revision {
                revision.validate()?;
            }
            let lexical_current = h.metadata_ready
                && h.catalog_ready
                && h.lexical_manifest
                    .as_ref()
                    .is_some_and(|m| m.matches(h.silver_revision.as_ref()));
            let gold_current = h.gold_manifest.as_ref().is_some_and(|m| {
                m.matches(
                    &self.build,
                    &identity.generation,
                    h.silver_revision.as_ref(),
                )
            }) && Some(h.counts.embeddings)
                == h.counts.expected_dense(&self.build);
            nodes.push(NodeFreshness {
                artifacts: Some(Artifacts {
                    // A retained node head exists. Its metadata projection may
                    // still need repair independently of lexical/dense gold.
                    metadata: State::publication(true, h.metadata_ready && h.catalog_ready),
                    lexical: State::publication(h.lexical_manifest.is_some(), lexical_current),
                    dense: if self.build.dense_enabled() {
                        State::publication(h.gold_manifest.is_some(), gold_current)
                    } else {
                        State::Disabled
                    },
                }),
                lexical_manifest: h.lexical_manifest.clone(),
                lexical_current,
                gold_manifest: h.gold_manifest.clone(),
                gold_current,
                silver_coverage: h.silver_coverage.clone(),
                silver_revision: h.silver_revision.clone(),
                source_silver: h.source_silver.clone(),
                node_id: node_id.clone(),
                revision: h.revision,
                last_synced_at: h.last_synced_at.clone(),
                last_updated_at: h.last_updated_at.clone(),
                conversations: i64::try_from(h.counts.conversations).unwrap_or(i64::MAX),
                messages: i64::try_from(h.counts.messages).unwrap_or(i64::MAX),
            });
        }
        self.stable_heads(identity, &heads).await?;
        self.unchanged_gold(identity, &group_gold.version).await?;
        Ok(Freshness {
            group_id: identity.group_id.clone(),
            unpublished_nodes: None,
            group_gold: Some(Box::new(group_gold)),
            nodes,
        })
    }
    pub(in crate::shared) async fn state(
        &self,
        identity: &Identity,
    ) -> Result<protocol::NodeState, AppError> {
        let loaded = self.load(identity).await?;
        let mut state = loaded.data.state(identity, &self.build);
        state.gold_build.clone_from(&self.build);
        state.publication_version = loaded.expected;
        if let Some(h) = loaded.head {
            let recipe_ready = h.gold_manifest.as_ref().is_some_and(|manifest| {
                manifest.matches(
                    &self.build,
                    &identity.generation,
                    h.silver_revision.as_ref(),
                )
            });
            if let Some(revision) = &h.silver_revision {
                revision.validate()?;
            }
            state.silver_coverage = h.silver_coverage;
            state.silver_revision = h.silver_revision;
            state.instance = Some(h.instance);
            state.revision = h.revision;
            state.source_silver = h.source_silver;
            state.silver_scans = h.silver_scans;
            if h.generation != identity.generation || !recipe_ready {
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
                            api::matches("version", head.versions()),
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
                    || head.thread_version(&row.id) != Some(payload.version.as_str())
                    || payload.node_id != *node
                    || point.id != point_id(&["catalog", &payload.version, &row.id])
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
        validate_search(&input, identity)?;
        if input.intent == crate::gold::exact::Intent::ExactMention {
            return tokio::time::timeout(std::time::Duration::from_millis(1500), self.search_exact(identity, input))
                .await.map_err(|_| AppError::usage("exact query exceeded the shared search budget; narrow the query or metadata filters"))?;
        }
        self.build.require_intent(input.intent)?;
        // Use one durable snapshot for both visibility and retrieval policy.
        // The post-query check still rejects a concurrently replaced node head.
        let configuration = self.active_configuration(identity).await?;
        let vector_policy = input.vector_policy.unwrap_or(configuration.vector_search);
        let heads = super::configuration::catalogue_nodes(configuration)?;
        let active: BTreeMap<_, _> = heads
            .iter()
            .filter(|(n, _)| !input.exclude_self || **n != identity.node_id)
            .collect();
        check_recipe(&heads, identity, &self.build)?;
        if active.values().any(|node| {
            node.head.as_ref().is_none_or(|h| {
                h.generation != identity.generation
                    || Some(h.counts.embeddings) != h.counts.expected_dense(&self.build)
            })
        }) {
            return Err(AppError::shared(
                "shared gold is stale, incomplete or incompatible; run mash index --rebuild-shared-gold on publishing nodes",
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
        self.view_at(&other, input, std::sync::Arc::new(head)).await
    }

    /// Context batches reuse one validated catalogue without caching it across
    /// requests. Each result still checks its node publication before release.
    pub(in crate::shared) async fn view_at(
        &self,
        identity: &Identity,
        input: protocol::View,
        head: std::sync::Arc<NodeHead>,
    ) -> Result<Vec<Message>, AppError> {
        if head.head.is_none() {
            return Ok(vec![]);
        }
        let name = collection(&identity.group_id, false);
        let records = if head.threads.is_some() {
            self.api
                .scroll::<Document>(
                    &name,
                    api::Filter {
                        must: vec![
                            api::matches("version", head.versions()),
                            api::matches("message.id", vec![input.id.clone()]),
                            api::matches("node_id", vec![input.node_id.clone()]),
                        ],
                        ..api::Filter::default()
                    },
                )
                .await?
        } else {
            self.api
                .retrieve::<Document>(&name, &[point_id(&["message", &head.version, &input.id])])
                .await?
        };
        let Some(anchor) = super::context::anchor(records, &head, &input)? else {
            self.unchanged(identity, &head.version).await?;
            return Ok(vec![]);
        };
        let thread_version = head
            .thread_version(&anchor.conversation_id)
            .ok_or_else(failure)?
            .to_owned();
        let records = super::context::neighborhood(
            &self.api,
            &name,
            api::Filter {
                must: vec![
                    api::matches("version", vec![thread_version.clone()]),
                    api::matches("node_id", vec![input.node_id.clone()]),
                    api::matches("conversation_id", vec![anchor.conversation_id.clone()]),
                ],
                ..api::Filter::default()
            },
            &anchor,
            input.context,
        )
        .await?;
        let (selection, head, input) = crate::blocking(move || {
            super::context::select(&anchor, records, &head, &input)
                .map(|selection| (selection, head, input))
        })
        .await?;
        let mut records = Vec::new();
        for ids in selection.ids(&thread_version).chunks(100) {
            records.extend(self.api.retrieve::<Document>(&name, ids).await?);
        }
        self.unchanged(identity, &head.version).await?;
        crate::blocking(move || selection.messages(records, &head, &input)).await
    }
}
fn hits(
    points: Vec<api::Scored<Document>>,
    lexical: bool,
    identity: &Identity,
    active: &BTreeMap<&String, &NodeHead>,
    build: &crate::gold::config::Build,
) -> Result<Vec<SearchHit>, AppError> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for point in points {
        let d = point.payload;
        let m = d.message;
        if point.id != point_id(&["message", &d.version, &m.id])
            || active
                .get(&d.node_id)
                .is_none_or(|h| h.thread_version(&m.conversation_id) != Some(d.version.as_str()))
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
