//! Per-node gold state and in-place document synchronization. No staging versions:
//! silver in SQLite is authoritative and any lost or partial gold is replayed.
use super::{
    AppError, ConnectorDocument, ConnectorUnit, Document, Identity, PointKind, Store, api,
    collection, connector_id, document_id, failure, point_id, sparse, tokens,
};
use crate::shared::protocol::{ConversationRecord, MessageRecord};
use crate::shared::silver_revision::State;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What one node's gold currently reflects. `silver` is set only after every
/// chat of that silver revision has documents built with this recipe.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(in crate::shared) struct NodeGold {
    kind: PointKind,
    pub(in crate::shared) node_id: String,
    pub(in crate::shared) build: crate::gold::config::Build,
    pub(in crate::shared) generation: String,
    pub(in crate::shared) silver: Option<State>,
    /// The last completed synchronization covered every chat with complete
    /// documents. Searches keep serving it while a later synchronization runs.
    #[serde(default)]
    pub(in crate::shared) certified: bool,
    pub(in crate::shared) chats: BTreeMap<String, GoldChat>,
    /// Connector unit anchor to the hex digest of its indexed text, for every
    /// connector point written from this node's shared silver.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(in crate::shared) connectors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(in crate::shared) struct GoldChat {
    /// Silver dependency fingerprint of the documents written for this chat.
    pub(in crate::shared) fingerprint: String,
    pub(in crate::shared) embeddings: u64,
    /// Every dense-eligible message of the chat has a vector.
    pub(in crate::shared) complete: bool,
}

impl NodeGold {
    pub(in crate::shared) fn empty(store: &Store, identity: &Identity) -> Self {
        Self {
            kind: PointKind::Gold,
            node_id: identity.node_id.clone(),
            build: store.build.clone(),
            generation: identity.generation.clone(),
            silver: None,
            certified: false,
            chats: BTreeMap::new(),
            connectors: BTreeMap::new(),
        }
    }

    /// Built with this group's recipe and model space.
    pub(in crate::shared) fn compatible(&self, store: &Store, identity: &Identity) -> bool {
        self.build == store.build && self.generation == identity.generation
    }

    /// Complete for exactly this silver revision.
    pub(in crate::shared) fn current(
        &self,
        store: &Store,
        identity: &Identity,
        silver: Option<&State>,
    ) -> bool {
        self.compatible(store, identity)
            && silver.is_some()
            && self.silver.as_ref() == silver
            && self.chats.values().all(|chat| chat.complete)
    }

    /// Dense retrieval may use this node's documents.
    pub(in crate::shared) fn searchable(&self, store: &Store, identity: &Identity) -> bool {
        self.compatible(store, identity) && self.certified
    }

    pub(in crate::shared) fn embeddings(&self) -> u64 {
        self.chats.values().map(|chat| chat.embeddings).sum()
    }
}

fn gold_id(node: &str) -> String {
    point_id(&["node-gold", node])
}

impl Store {
    pub(in crate::shared) async fn node_gold(
        &self,
        identity: &Identity,
    ) -> Result<Option<NodeGold>, AppError> {
        let id = gold_id(&identity.node_id);
        let mut points = self
            .api
            .retrieve::<NodeGold>(
                &collection(&identity.group_id, true),
                std::slice::from_ref(&id),
            )
            .await?;
        let gold = points.pop().map(|point| point.payload);
        if gold
            .as_ref()
            .is_some_and(|gold| gold.kind != PointKind::Gold || gold.node_id != identity.node_id)
        {
            return Err(failure());
        }
        Ok(gold)
    }

    pub(in crate::shared) async fn put_node_gold(
        &self,
        identity: &Identity,
        gold: &NodeGold,
    ) -> Result<(), AppError> {
        self.api
            .upsert(
                &collection(&identity.group_id, true),
                &api::Upsert {
                    points: vec![api::Point {
                        id: gold_id(&identity.node_id),
                        vector: api::Vectors::default(),
                        payload: gold,
                    }],
                },
            )
            .await
    }

    /// Upsert one batch of a chat's documents; returns stored dense vectors.
    /// Delete a chat's documents except the listed current messages.
    pub(in crate::shared) async fn retain_documents(
        &self,
        identity: &Identity,
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

    /// Delete one node's connector points with these anchors.
    pub(in crate::shared) async fn delete_connector_points(
        &self,
        identity: &Identity,
        anchors: Vec<String>,
    ) -> Result<(), AppError> {
        if anchors.is_empty() {
            return Ok(());
        }
        self.api
            .delete_filtered(
                &collection(&identity.group_id, false),
                api::Filter {
                    must: vec![
                        api::matches("node_id", vec![identity.node_id.clone()]),
                        api::matches("unit.anchor", anchors),
                    ],
                    ..api::Filter::default()
                },
            )
            .await
    }

    /// Delete all documents of one node, for a complete replay.
    pub(in crate::shared) async fn delete_node_documents(
        &self,
        identity: &Identity,
    ) -> Result<(), AppError> {
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

    /// Drain optimization started by synchronization, then exercise retrieval.
    pub(in crate::shared) async fn settle(&self, identity: &Identity) -> Result<(), AppError> {
        let name = collection(&identity.group_id, false);
        self.api.settle_search(&name).await?;
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

/// Buffered gold documents for one node; `finish` sends the remainder.
pub(in crate::shared) struct Documents {
    name: String,
    batch: api::StagingBatch,
}

impl Documents {
    /// A writer that fills whole Qdrant requests across chats.
    pub(in crate::shared) fn new(identity: &Identity) -> Self {
        Self {
            name: collection(&identity.group_id, false),
            batch: api::StagingBatch::default(),
        }
    }

    /// Queue one message's document; returns 1 when it carries an embedding.
    pub(in crate::shared) async fn push(
        &mut self,
        store: &Store,
        identity: &Identity,
        conversation: &ConversationRecord,
        message: MessageRecord,
    ) -> Result<u64, AppError> {
        if message.conversation_id != conversation.id {
            return Err(failure());
        }
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

    /// Queue one connector unit's point, with its vector when dense gold is built.
    pub(in crate::shared) async fn push_connector(
        &mut self,
        store: &Store,
        identity: &Identity,
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

    pub(in crate::shared) async fn finish(mut self, store: &Store) -> Result<(), AppError> {
        self.batch.flush(&store.api, &self.name).await
    }
}

/// One connector unit ready to write.
pub(in crate::shared) struct ConnectorPoint {
    pub(in crate::shared) source_id: String,
    pub(in crate::shared) object: crate::source::ObjectType,
    pub(in crate::shared) group_anchor: String,
    pub(in crate::shared) created_at: Option<i64>,
    pub(in crate::shared) unit: ConnectorUnit,
    pub(in crate::shared) vector: Option<Vec<f64>>,
}
