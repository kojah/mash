//! Per-node views state and in-place document synchronization. No staging versions:
//! silver in SQLite is authoritative and any lost or partial views are replayed.
use super::{
    AppError, Document, PointKind, Scope, SearchUnit, Store, api, collection, document_id, failure,
    point_id, tokens,
};
use crate::silver::revision::State;
use crate::views::documents::{ChatRecord, MessageRecord, SearchMessage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What one node's views currently reflects. `silver` is set only after every
/// chat of that silver revision has documents built with this recipe; a local
/// pass whose inference failed sets it with some documents lacking vectors,
/// which `certified` and each chat's evidence then record.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NodeViews {
    kind: PointKind,
    pub node_id: String,
    pub build: crate::views::config::Build,
    pub generation: String,
    pub silver: Option<State>,
    /// The last completed synchronization covered every chat with complete
    /// documents. Searches keep serving it while a later synchronization runs.
    #[serde(default)]
    pub certified: bool,
    /// Silver revision these chats were built from. Certification only claims a
    /// head equal to it, so views never report the current head while serving a
    /// different revision's content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    pub projections: super::Projections,
}

impl NodeViews {
    pub fn empty(store: &Store, identity: &Scope) -> Self {
        Self {
            kind: PointKind::Views,
            node_id: identity.node_id.clone(),
            build: store.build.clone(),
            generation: identity.generation.clone(),
            silver: None,
            certified: false,
            input: None,
            projections: super::Projections::default(),
        }
    }

    /// Built with this group's recipe and model space.
    pub fn compatible(&self, store: &Store, identity: &Scope) -> bool {
        self.build == store.build && self.generation == identity.generation
    }

    /// Every chat of exactly this silver revision has its documents, so word
    /// search is current. A failed inference can leave some without vectors.
    pub fn words_current(&self, store: &Store, identity: &Scope, silver: Option<&State>) -> bool {
        self.compatible(store, identity) && silver.is_some() && self.silver.as_ref() == silver
    }

    /// Complete for exactly this silver revision, vectors included.
    pub fn current(&self, store: &Store, identity: &Scope, silver: Option<&State>) -> bool {
        self.words_current(store, identity, silver) && self.projections.complete()
    }

    /// Dense retrieval may use this node's documents.
    pub fn searchable(&self, store: &Store, identity: &Scope) -> bool {
        self.compatible(store, identity) && self.certified
    }

    pub fn embeddings(&self) -> u64 {
        self.projections
            .chats()
            .map(|(_, evidence)| evidence.embeddings)
            .sum()
    }

    pub fn stored_vectors(&self) -> u64 {
        self.projections.embeddings()
    }
}

pub(super) fn views_id(node: &str) -> String {
    point_id(&["node-views", node])
}

#[derive(Deserialize)]
struct CachedEmbedding {
    node_id: String,
    provider: crate::provider::Provider,
    unit: CachedUnit,
    vector: Option<Vec<f64>>,
    embedding_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct CachedUnit {
    anchor: String,
    generation: Option<String>,
}

/// What a message's stored document must say for a refresh to leave it alone.
pub struct Expected<'a> {
    pub message: &'a SearchMessage,
    /// The prepared dense input, when the recipe embeds this message.
    pub dense: Option<&'a str>,
}

/// The part of a stored document a refresh compares. Every field is optional
/// so a document missing one simply differs and is written again.
#[derive(Deserialize)]
struct StoredDocument {
    node_id: Option<String>,
    provider: Option<crate::provider::Provider>,
    role: Option<crate::role::Role>,
    chat_id: Option<String>,
    project: Option<String>,
    repository: Option<String>,
    created_at: Option<i64>,
    unit: Option<StoredUnit>,
    embedding_fingerprint: Option<String>,
}

#[derive(Deserialize)]
struct StoredUnit {
    anchor: Option<String>,
    text: Option<String>,
    sequence: Option<i64>,
    generation: Option<String>,
    title: Option<String>,
    enrichment: Option<serde::de::IgnoredAny>,
    chat_created_at: Option<i64>,
    chat_updated_at: Option<i64>,
}

/// Documents compared per request; only their text is sizeable.
const COMPARE_ROWS: usize = 1024;

impl Store {
    /// Which of a chat's messages already have exactly the document a refresh
    /// would write. Those are left alone: a long chat gains a message without
    /// its other documents being sent again. The dense vector itself is not
    /// read; its fingerprint ties it to the same input and model, and search
    /// still checks every vector it returns.
    pub async fn unchanged_documents(
        &self,
        scope: &Scope,
        chat: &ChatRecord,
        expected: &[Expected<'_>],
    ) -> Result<Vec<bool>, AppError> {
        let mut unchanged = Vec::with_capacity(expected.len());
        for batch in expected.chunks(COMPARE_ROWS) {
            let ids = batch
                .iter()
                .map(|row| document_id(&scope.node_id, &row.message.id))
                .collect::<Vec<_>>();
            let mut stored = self
                .api
                .retrieve_fields::<StoredDocument>(
                    &collection(&scope.group_id, false),
                    &ids,
                    &[
                        "node_id",
                        "provider",
                        "role",
                        "chat_id",
                        "project",
                        "repository",
                        "created_at",
                        "unit.anchor",
                        "unit.text",
                        "unit.sequence",
                        "unit.generation",
                        "unit.title",
                        "unit.enrichment",
                        "unit.chat_created_at",
                        "unit.chat_updated_at",
                        "embedding_fingerprint",
                    ],
                )
                .await?
                .into_iter()
                .map(|point| (point.id, point.payload))
                .collect::<BTreeMap<_, _>>();
            for (row, id) in batch.iter().zip(ids) {
                unchanged.push(
                    stored
                        .remove(&id)
                        .is_some_and(|document| document.is(scope, chat, row)),
                );
            }
        }
        Ok(unchanged)
    }
}

impl StoredDocument {
    fn is(&self, scope: &Scope, chat: &ChatRecord, expected: &Expected<'_>) -> bool {
        let message = expected.message;
        let Some(unit) = &self.unit else {
            return false;
        };
        let dense = expected.dense.map(|text| {
            crate::views::documents::EmbeddingInput {
                provider: chat.provider,
                anchor: &message.id,
                text,
            }
            .fingerprint(&scope.generation, scope.dimensions)
        });
        self.node_id.as_deref() == Some(scope.node_id.as_str())
            && self.provider == Some(chat.provider)
            && self.role.as_ref() == Some(&message.role)
            && self.chat_id.as_deref() == Some(chat.id.as_str())
            && self.project == chat.project
            && self.repository == chat.repository
            && self.created_at == message.created_at
            && unit.anchor.as_deref() == Some(message.id.as_str())
            && unit.text.as_deref() == Some(message.projection.as_str())
            && unit.sequence == Some(message.ordinal)
            && unit.generation.as_deref() == dense.as_ref().map(|_| scope.generation.as_str())
            && self.embedding_fingerprint == dense
            // A document from before chat facts moved to silver is rewritten
            // once, without them.
            && unit.title.is_none()
            && unit.enrichment.is_none()
            && unit.chat_created_at.is_none()
            && unit.chat_updated_at.is_none()
    }
}

impl Store {
    /// Read bounded reusable dense evidence. An uncertified point may supply a
    /// vector, but never metadata or certification: the caller writes fresh
    /// canonical evidence and completes the normal certification sequence.
    pub async fn reusable_vectors(
        &self,
        scope: &Scope,
        inputs: &[crate::views::documents::EmbeddingInput<'_>],
    ) -> Result<Vec<Option<Vec<f64>>>, AppError> {
        let mut result = Vec::with_capacity(inputs.len());
        for batch in inputs.chunks(crate::views::shared::CACHE_ROWS) {
            let ids = batch
                .iter()
                .map(|input| document_id(&scope.node_id, input.anchor))
                .collect::<Vec<_>>();
            let mut points = self
                .api
                .retrieve_fields::<CachedEmbedding>(
                    &collection(&scope.group_id, false),
                    &ids,
                    &[
                        "node_id",
                        "provider",
                        "unit.anchor",
                        "unit.generation",
                        "vector",
                        "embedding_fingerprint",
                    ],
                )
                .await?
                .into_iter()
                .map(|point| (point.id, point.payload))
                .collect::<BTreeMap<_, _>>();
            for (input, id) in batch.iter().zip(ids) {
                let vector = points
                    .remove(&id)
                    .filter(|point| {
                        point.node_id == scope.node_id
                            && point.provider == input.provider
                            && point.unit.anchor == input.anchor
                            && point.unit.generation.as_deref() == Some(scope.generation.as_str())
                            && point.embedding_fingerprint.as_deref()
                                == Some(
                                    input
                                        .fingerprint(&scope.generation, scope.dimensions)
                                        .as_str(),
                                )
                            && point.vector.as_ref().is_some_and(|v| {
                                crate::views::retrieval::valid_vector(v, scope.dimensions)
                            })
                    })
                    .and_then(|point| point.vector);
                result.push(vector);
            }
        }
        Ok(result)
    }

    pub async fn node_views(&self, identity: &Scope) -> Result<Option<NodeViews>, AppError> {
        let id = views_id(&identity.node_id);
        let mut points = self
            .api
            .retrieve::<NodeViews>(
                &collection(&identity.group_id, true),
                std::slice::from_ref(&id),
            )
            .await?;
        let views = points.pop().map(|point| point.payload);
        if views.as_ref().is_some_and(|views| {
            views.kind != PointKind::Views || views.node_id != identity.node_id
        }) {
            return Err(failure());
        }
        Ok(views)
    }

    pub async fn put_node_views(
        &self,
        identity: &Scope,
        views: &NodeViews,
    ) -> Result<(), AppError> {
        self.put_node_views_with_wait(identity, views, true).await
    }

    pub async fn put_node_views_unwaited(
        &self,
        identity: &Scope,
        views: &NodeViews,
    ) -> Result<(), AppError> {
        self.put_node_views_with_wait(identity, views, false).await
    }

    async fn put_node_views_with_wait(
        &self,
        identity: &Scope,
        views: &NodeViews,
        wait: bool,
    ) -> Result<(), AppError> {
        self.api
            .upsert_with_wait(
                &collection(&identity.group_id, true),
                &api::Upsert {
                    points: vec![api::Point {
                        id: views_id(&identity.node_id),
                        vector: api::Vectors::default(),
                        payload: views,
                    }],
                },
                wait,
            )
            .await
    }

    /// Delete a chat's documents except the listed current messages.
    pub async fn retain_documents(
        &self,
        identity: &Scope,
        chat: &str,
        current: Vec<String>,
    ) -> Result<(), AppError> {
        let mut filter = api::Filter {
            must: vec![
                api::matches("node_id", vec![identity.node_id.clone()]),
                api::matches("chat_id", vec![chat.to_owned()]),
            ],
            ..api::Filter::default()
        };
        if !current.is_empty() {
            filter.must_not.push(api::matches("unit.anchor", current));
        }
        self.api
            .delete_filtered(&collection(&identity.group_id, false), filter)
            .await
    }

    /// Delete these messages' documents from one chat, leaving the others.
    pub async fn remove_documents(
        &self,
        identity: &Scope,
        chat: &str,
        messages: Vec<String>,
    ) -> Result<(), AppError> {
        if messages.is_empty() {
            return Ok(());
        }
        let filter = api::Filter {
            must: vec![
                api::matches("node_id", vec![identity.node_id.clone()]),
                api::matches("chat_id", vec![chat.to_owned()]),
                api::matches("unit.anchor", messages),
            ],
            ..api::Filter::default()
        };
        self.api
            .delete_filtered(&collection(&identity.group_id, false), filter)
            .await
    }

    /// Reconcile native deletions even when previous request metadata was lost.
    pub async fn retain_node_chats(
        &self,
        identity: &Scope,
        current: Vec<String>,
    ) -> Result<(), AppError> {
        self.retain_node_chats_with_wait(identity, current, true)
            .await
    }

    pub async fn retain_node_chats_unwaited(
        &self,
        identity: &Scope,
        current: Vec<String>,
    ) -> Result<(), AppError> {
        self.retain_node_chats_with_wait(identity, current, false)
            .await
    }

    async fn retain_node_chats_with_wait(
        &self,
        identity: &Scope,
        current: Vec<String>,
        wait: bool,
    ) -> Result<(), AppError> {
        let mut filter = api::Filter {
            must: vec![api::matches("node_id", vec![identity.node_id.clone()])],
            ..api::Filter::default()
        };
        if !current.is_empty() {
            filter.must_not.push(api::matches("chat_id", current));
        }
        self.api
            .delete_filtered_with_wait(&collection(&identity.group_id, false), filter, wait)
            .await
    }

    /// Delete all documents of one node, for a complete replay.
    pub async fn delete_node_documents(&self, identity: &Scope) -> Result<(), AppError> {
        self.delete_node_documents_with_wait(identity, true).await
    }

    pub async fn delete_node_documents_unwaited(&self, identity: &Scope) -> Result<(), AppError> {
        self.delete_node_documents_with_wait(identity, false).await
    }

    async fn delete_node_documents_with_wait(
        &self,
        identity: &Scope,
        wait: bool,
    ) -> Result<(), AppError> {
        self.api
            .delete_filtered_with_wait(
                &collection(&identity.group_id, false),
                api::Filter {
                    must: vec![api::matches("node_id", vec![identity.node_id.clone()])],
                    ..api::Filter::default()
                },
                wait,
            )
            .await
    }

    /// Check health and exercise retrieval after awaited document writes.
    pub async fn settle(&self, identity: &Scope) -> Result<(), AppError> {
        let name = collection(&identity.group_id, false);
        self.api.check_search_health(&name).await?;
        if self.build.dense_enabled() {
            let configuration = self.active_configuration(identity).await?;
            self.api
                .warm_search(
                    &name,
                    self.dense_name(identity),
                    identity.dimensions,
                    std::slice::from_ref(&identity.node_id),
                    configuration.vector_search,
                )
                .await?;
        }
        Ok(())
    }

    /// Defer HNSW construction while a bulk index is staging documents.
    pub async fn begin_bulk(&self, identity: &Scope) -> Result<(), AppError> {
        if !self.build.dense_enabled() {
            return Ok(());
        }
        self.api
            .begin_bulk(&collection(&identity.group_id, false))
            .await
    }

    /// Lower the threshold after all writes, scheduling one graph build.
    /// Exact-only local views do not need a graph at all.
    pub async fn end_bulk(&self, identity: &Scope) -> Result<(), AppError> {
        if !self.build.dense_enabled()
            || !matches!(self.vector_index, super::VectorIndex::Hnsw { .. })
        {
            return Ok(());
        }
        self.api
            .end_bulk(&collection(&identity.group_id, false))
            .await
    }
}

/// Buffered views documents for one node; `finish` sends the remainder.
pub struct Documents {
    name: String,
    batch: api::StagingBatch,
}

impl Documents {
    /// A writer that fills whole Qdrant requests across chats.
    pub fn new(identity: &Scope) -> Self {
        Self {
            name: collection(&identity.group_id, false),
            batch: api::StagingBatch::default(),
        }
    }

    /// Let a bulk caller queue unwaited writes; the final state point is awaited.
    pub fn with_wait(mut self, wait: bool) -> Self {
        self.batch.set_wait(wait);
        self
    }

    /// Queue one message's document; returns 1 when it carries an embedding.
    pub async fn push(
        &mut self,
        store: &Store,
        identity: &Scope,
        chat: &ChatRecord,
        message: MessageRecord,
    ) -> Result<u64, AppError> {
        self.push_native(store, identity, chat, message.into(), None)
            .await
    }

    pub async fn push_native(
        &mut self,
        store: &Store,
        identity: &Scope,
        chat: &ChatRecord,
        message: SearchMessage,
        embedding_fingerprint: Option<String>,
    ) -> Result<u64, AppError> {
        if message.chat_id != chat.id {
            return Err(failure());
        }
        // Canonical context remains in silver. An empty projection contributes
        // neither lexical nor dense evidence and needs no search document.
        // Coordinators reconcile the nonempty IDs to remove formerly visible rows.
        if message.projection.is_empty() {
            return Ok(0);
        }
        let words = tokens(&message.projection);
        let embedded = u64::from(message.vector.is_some());
        let vector = api::Vectors {
            dense: message
                .vector
                .clone()
                .map(|v| (store.dense_name(identity).into(), v)),
        };
        self.push_point(
            store,
            api::Point {
                id: document_id(&identity.node_id, &message.id),
                vector,
                payload: Document {
                    node_id: identity.node_id.clone(),
                    provider: chat.provider,
                    role: message.role,
                    chat_id: chat.id.clone(),
                    project: chat.project.clone(),
                    repository: chat.repository.clone(),
                    created_at: message.created_at,
                    tokens: words,
                    unit: SearchUnit {
                        anchor: message.id,
                        text: message.projection.clone(),
                        content: message.projection,
                        sequence: message.ordinal,
                        title: None,
                        enrichment: None,
                        chat_created_at: None,
                        chat_updated_at: None,
                        generation: message.generation,
                    },
                    vector: message.vector,
                    embedding_fingerprint,
                },
            },
        )
        .await?;
        Ok(embedded)
    }

    /// All source projections share the same bounded, completion-checked writer.
    async fn push_point<P: Serialize>(
        &mut self,
        store: &Store,
        point: api::Point<P>,
    ) -> Result<(), AppError> {
        self.batch.push(&store.api, &self.name, point).await
    }

    pub async fn finish(mut self, store: &Store) -> Result<(), AppError> {
        self.flush(store).await
    }

    pub async fn flush(&mut self, store: &Store) -> Result<(), AppError> {
        self.batch.flush(&store.api, &self.name).await
    }
}
