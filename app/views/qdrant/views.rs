//! Per-node views state and in-place document synchronization. No staging versions:
//! silver in SQLite is authoritative and any lost or partial views are replayed.
use super::{
    AppError, Document, PointKind, Scope, SearchUnit, Store, api, collection, document_id, failure,
    point_id, sparse, tokens,
};
use crate::silver::revision::State;
use crate::views::documents::{ChatRecord, MessageRecord, SearchMessage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What one node's views currently reflects. `silver` is set only after every
/// chat of that silver revision has documents built with this recipe.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeViews {
    kind: PointKind,
    pub(crate) node_id: String,
    pub(crate) build: crate::views::config::Build,
    pub(crate) generation: String,
    pub(crate) silver: Option<State>,
    /// The last completed synchronization covered every chat with complete
    /// documents. Searches keep serving it while a later synchronization runs.
    #[serde(default)]
    pub(crate) certified: bool,
    /// Silver revision these chats were built from. Certification only claims a
    /// head equal to it, so views never report the current head while serving a
    /// different revision's content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) input: Option<i64>,
    pub(crate) projections: super::Projections,
}

impl NodeViews {
    pub(crate) fn empty(store: &Store, identity: &Scope) -> Self {
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
    pub(crate) fn compatible(&self, store: &Store, identity: &Scope) -> bool {
        self.build == store.build && self.generation == identity.generation
    }

    /// Complete for exactly this silver revision.
    pub(crate) fn current(&self, store: &Store, identity: &Scope, silver: Option<&State>) -> bool {
        self.compatible(store, identity)
            && silver.is_some()
            && self.silver.as_ref() == silver
            && self.projections.complete()
    }

    /// Dense retrieval may use this node's documents.
    pub(crate) fn searchable(&self, store: &Store, identity: &Scope) -> bool {
        self.compatible(store, identity) && self.certified
    }

    pub(crate) fn embeddings(&self) -> u64 {
        self.projections
            .chats()
            .map(|(_, evidence)| evidence.embeddings)
            .sum()
    }

    pub(crate) fn stored_vectors(&self) -> u64 {
        self.projections.embeddings()
    }
}

fn views_id(node: &str) -> String {
    point_id(&["node-views", node])
}

impl Store {
    /// Read bounded reusable dense evidence. An uncertified point may supply a
    /// vector, but never metadata or certification: the caller writes fresh
    /// canonical evidence and completes the normal certification sequence.
    pub(crate) async fn reusable_vectors(
        &self,
        scope: &Scope,
        inputs: &[crate::views::documents::EmbeddingInput<'_>],
    ) -> Result<Vec<Option<Vec<f64>>>, AppError> {
        let mut result = Vec::with_capacity(inputs.len());
        for batch in inputs.chunks(crate::views::shared::BATCH_ROWS) {
            let ids = batch
                .iter()
                .map(|input| document_id(&scope.node_id, input.anchor))
                .collect::<Vec<_>>();
            let mut points = self
                .api
                .retrieve::<Document>(&collection(&scope.group_id, false), &ids)
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

    pub(crate) async fn node_views(&self, identity: &Scope) -> Result<Option<NodeViews>, AppError> {
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

    pub(crate) async fn put_node_views(
        &self,
        identity: &Scope,
        views: &NodeViews,
    ) -> Result<(), AppError> {
        self.api
            .upsert(
                &collection(&identity.group_id, true),
                &api::Upsert {
                    points: vec![api::Point {
                        id: views_id(&identity.node_id),
                        vector: api::Vectors::default(),
                        payload: views,
                    }],
                },
            )
            .await
    }

    /// Delete a chat's documents except the listed current messages.
    pub(crate) async fn retain_documents(
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

    /// Reconcile native deletions even when previous request metadata was lost.
    pub(crate) async fn retain_node_chats(
        &self,
        identity: &Scope,
        current: Vec<String>,
    ) -> Result<(), AppError> {
        let mut filter = api::Filter {
            must: vec![api::matches("node_id", vec![identity.node_id.clone()])],
            ..api::Filter::default()
        };
        if !current.is_empty() {
            filter.must_not.push(api::matches("chat_id", current));
        }
        self.api
            .delete_filtered(&collection(&identity.group_id, false), filter)
            .await
    }

    /// Delete all documents of one node, for a complete replay.
    pub(crate) async fn delete_node_documents(&self, identity: &Scope) -> Result<(), AppError> {
        self.api
            .delete_filtered(
                &collection(&identity.group_id, false),
                api::Filter {
                    must: vec![api::matches("node_id", vec![identity.node_id.clone()])],
                    ..api::Filter::default()
                },
            )
            .await
    }

    /// Check health and exercise retrieval after awaited document writes.
    pub(crate) async fn settle(&self, identity: &Scope) -> Result<(), AppError> {
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
}

/// Buffered views documents for one node; `finish` sends the remainder.
pub(crate) struct Documents {
    name: String,
    batch: api::StagingBatch,
}

impl Documents {
    /// A writer that fills whole Qdrant requests across chats.
    pub(crate) fn new(identity: &Scope) -> Self {
        Self {
            name: collection(&identity.group_id, false),
            batch: api::StagingBatch::default(),
        }
    }

    /// Queue one message's document; returns 1 when it carries an embedding.
    pub(crate) async fn push(
        &mut self,
        store: &Store,
        identity: &Scope,
        chat: &ChatRecord,
        message: MessageRecord,
    ) -> Result<u64, AppError> {
        self.push_native(store, identity, chat, message.into(), None)
            .await
    }

    pub(crate) async fn push_native(
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
        let vector = Self::vectors(
            store,
            identity,
            &message.projection,
            &words,
            message.vector.clone(),
        );
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
                        title: chat.title.clone(),
                        enrichment: chat.enrichment.clone(),
                        sequence: message.ordinal,
                        chat_created_at: chat.created_at,
                        chat_updated_at: chat.updated_at,
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

    fn vectors(
        store: &Store,
        identity: &Scope,
        text: &str,
        words: &[String],
        dense: Option<Vec<f64>>,
    ) -> api::Vectors {
        api::Vectors {
            dense: dense.map(|v| (store.dense_name(identity).into(), v)),
            lexical: (!words.is_empty()).then(|| sparse(text, false)),
        }
    }

    /// All source projections share the same bounded, completion-checked writer.
    async fn push_point<P: Serialize>(
        &mut self,
        store: &Store,
        point: api::Point<P>,
    ) -> Result<(), AppError> {
        self.batch.push(&store.api, &self.name, point).await
    }

    pub(crate) async fn finish(mut self, store: &Store) -> Result<(), AppError> {
        self.flush(store).await
    }

    pub(crate) async fn flush(&mut self, store: &Store) -> Result<(), AppError> {
        self.batch.flush(&store.api, &self.name).await
    }
}
