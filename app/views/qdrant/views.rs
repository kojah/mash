//! Per-node views state and in-place document synchronization. No staging versions:
//! silver in SQLite is authoritative and any lost or partial views are replayed.
use super::{
    AppError, ConnectorDocument, ConnectorUnit, Document, PointKind, Scope, Store, api, collection,
    connector_id, document_id, failure, point_id, sparse, tokens,
};
use crate::silver::revision::State;
use crate::views::documents::{ConversationRecord, MessageRecord};
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
    pub(crate) chats: BTreeMap<String, ViewChat>,
    /// Connector unit anchor to the hex digest of its indexed text, for every
    /// connector point written from this node's shared silver.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) connectors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ViewChat {
    /// Silver dependency fingerprint of the documents written for this chat.
    pub(crate) fingerprint: String,
    pub(crate) embeddings: u64,
    /// Every dense-eligible message of the chat has a vector.
    pub(crate) complete: bool,
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
            chats: BTreeMap::new(),
            connectors: BTreeMap::new(),
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
            && self.chats.values().all(|chat| chat.complete)
    }

    /// Dense retrieval may use this node's documents.
    pub(crate) fn searchable(&self, store: &Store, identity: &Scope) -> bool {
        self.compatible(store, identity) && self.certified
    }

    pub(crate) fn embeddings(&self) -> u64 {
        self.chats.values().map(|chat| chat.embeddings).sum()
    }
}

fn views_id(node: &str) -> String {
    point_id(&["node-views", node])
}

impl Store {
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

    /// Upsert one batch of a chat's documents; returns stored dense vectors.
    /// Delete a chat's documents except the listed current messages.
    pub(crate) async fn retain_documents(
        &self,
        identity: &Scope,
        conversation: &str,
        current: Vec<String>,
    ) -> Result<(), AppError> {
        let mut filter = api::Filter {
            must: vec![
                api::matches("node_id", vec![identity.node_id.clone()]),
                api::matches("conversation_id", vec![conversation.to_owned()]),
            ],
            ..api::Filter::default()
        };
        if !current.is_empty() {
            filter.must_not.push(api::matches("message.id", current));
        }
        self.api
            .delete_filtered(&collection(&identity.group_id, false), filter)
            .await
    }

    /// Remove obsolete connector points, including writes never recorded in metadata.
    pub(crate) async fn retain_connector_points(
        &self,
        identity: &Scope,
        current: Vec<String>,
    ) -> Result<(), AppError> {
        let mut filter = api::Filter {
            must: vec![
                api::matches("node_id", vec![identity.node_id.clone()]),
                api::Condition::IsEmpty {
                    is_empty: api::Field {
                        key: "provider".into(),
                    },
                },
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
    pub(crate) async fn retain_node_conversations(
        &self,
        identity: &Scope,
        current: Vec<String>,
    ) -> Result<(), AppError> {
        let mut filter = api::Filter {
            must: vec![
                api::matches("node_id", vec![identity.node_id.clone()]),
                // Connector points share this collection, but have no provider.
                api::matches(
                    "provider",
                    crate::provider::Provider::ALL
                        .iter()
                        .map(|p| p.as_str().to_owned())
                        .collect(),
                ),
            ],
            ..api::Filter::default()
        };
        if !current.is_empty() {
            filter
                .must_not
                .push(api::matches("conversation_id", current));
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
        conversation: &ConversationRecord,
        mut message: MessageRecord,
    ) -> Result<u64, AppError> {
        if message.conversation_id != conversation.id {
            return Err(failure());
        }
        // Retrieval documents need no tool activity; view reads shared silver.
        message.tools = crate::tools::Activity::default();
        let words = tokens(&message.projection);
        let embedded = u64::from(message.vector.is_some());
        self.batch
            .push(
                &store.api,
                &self.name,
                api::Point {
                    id: document_id(&identity.node_id, &message.id),
                    vector: api::Vectors {
                        dense: message
                            .vector
                            .clone()
                            .map(|v| (store.dense_name(identity).into(), v)),
                        lexical: (!words.is_empty()).then(|| sparse(&message.projection, false)),
                    },
                    payload: Document {
                        node_id: identity.node_id.clone(),
                        provider: conversation.provider,
                        role: message.role.clone(),
                        conversation_id: conversation.id.clone(),
                        project: conversation.project.clone(),
                        repository: conversation.repository.clone(),
                        created_at: message.created_at,
                        tokens: words,
                        conversation: conversation.clone(),
                        message,
                    },
                },
            )
            .await?;
        Ok(embedded)
    }

    /// Queue one connector unit's point, with its vector when dense views are built.
    pub(crate) async fn push_connector(
        &mut self,
        store: &Store,
        identity: &Scope,
        point: ConnectorPoint,
    ) -> Result<(), AppError> {
        let ConnectorPoint {
            source_id,
            object,
            group_anchor,
            created_at,
            mut unit,
            vector,
        } = point;
        let words = tokens(&unit.text);
        unit.generation = vector.as_ref().map(|_| identity.generation.clone());
        self.batch
            .push(
                &store.api,
                &self.name,
                api::Point {
                    id: connector_id(&identity.node_id, &unit.anchor),
                    vector: api::Vectors {
                        dense: vector.map(|v| (store.dense_name(identity).into(), v)),
                        lexical: (!words.is_empty()).then(|| sparse(&unit.text, false)),
                    },
                    payload: ConnectorDocument {
                        node_id: identity.node_id.clone(),
                        source_id,
                        object,
                        conversation_id: group_anchor,
                        created_at,
                        tokens: words,
                        unit,
                    },
                },
            )
            .await
    }

    pub(crate) async fn finish(mut self, store: &Store) -> Result<(), AppError> {
        self.batch.flush(&store.api, &self.name).await
    }
}

/// One connector unit ready to write.
pub(crate) struct ConnectorPoint {
    pub(crate) source_id: String,
    pub(crate) object: crate::source::ObjectType,
    pub(crate) group_anchor: String,
    pub(crate) created_at: Option<i64>,
    pub(crate) unit: ConnectorUnit,
    pub(crate) vector: Option<Vec<f64>>,
}
