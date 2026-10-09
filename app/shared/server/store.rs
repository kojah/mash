//! Shared service storage: SQLite silver is the system of record; Qdrant gold is
//! derived from it and can be rebuilt by replaying silver.
use super::Identity;
use super::configuration::{Configuration, env_name};
use crate::AppError;
use crate::shared::qdrant_index::{self, NodeGold};
use crate::shared::silver_store::{self, Head, SilverStore};
use crate::shared::{Freshness, NodeFreshness, protocol};
use crate::storage::Counts;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

mod connectors;
mod replay;
mod sync;

/// Serializes publications per node while other nodes proceed.
type NodeLock = Arc<tokio::sync::Mutex<()>>;

#[derive(Clone)]
pub(in crate::shared) struct Store {
    silver: SilverStore,
    gold: qdrant_index::Store,
    max_upload_bytes: u64,
    nodes: Arc<std::sync::Mutex<HashMap<(String, String), NodeLock>>>,
}

pub(super) async fn initialize(
    config: &Configuration,
    identities: &HashMap<blake3::Hash, Identity>,
    activate_gold: bool,
    activate_query: bool,
    activate_memory: bool,
) -> Result<Store, AppError> {
    let groups: HashSet<_> = identities.values().map(|id| id.group_id.clone()).collect();
    let groups = groups.into_iter().collect::<Vec<_>>();
    let name = config
        .qdrant_url_env
        .as_ref()
        .ok_or_else(|| AppError::configuration("Qdrant requires qdrant_url_env"))?;
    env_name(name)?;
    let url = std::env::var(name)
        .map_err(|_| AppError::configuration(format!("set {name} to the Qdrant HTTP endpoint")))?;
    let key = config
        .qdrant_api_key_env
        .as_ref()
        .map(|name| {
            env_name(name)?;
            std::env::var(name)
                .map_err(|_| AppError::configuration(format!("set {name} to the Qdrant API key")))
        })
        .transpose()?;
    let root = config.silver_dir()?;
    let configured = identities.values().cloned().collect::<Vec<_>>();
    let silver = crate::blocking(move || SilverStore::open(&root, &configured)).await?;
    let gold = qdrant_index::Store::connect(
        &url,
        key.as_deref(),
        &groups,
        config.embeddings.dimensions,
        RESPONSE_BYTES,
        config.gold.query.vector_search,
        qdrant_index::MemoryPolicy {
            tier: config.qdrant_memory,
            activate: activate_memory,
        },
    )
    .await?
    .with_build(config.gold.build.clone());
    let mut selected = HashSet::new();
    for identity in identities.values() {
        if selected.insert(&identity.group_id) {
            let group_store = gold.clone().with_query(
                config
                    .gold
                    .query
                    .groups
                    .get(&identity.group_id)
                    .copied()
                    .unwrap_or(config.gold.query.vector_search),
            );
            group_store.select_gold(identity, activate_gold).await?;
            group_store.select_query(identity, activate_query).await?;
        }
    }
    Ok(Store::new(silver, gold, config.max_upload_bytes()?))
}

/// Bound on one Qdrant response; documents are read a page at a time.
const RESPONSE_BYTES: usize = 64 * 1024 * 1024;

impl Store {
    pub(in crate::shared) fn new(
        silver: SilverStore,
        gold: qdrant_index::Store,
        max_upload_bytes: u64,
    ) -> Self {
        Self {
            silver,
            gold,
            max_upload_bytes,
            nodes: Arc::default(),
        }
    }

    #[cfg(test)]
    pub(in crate::shared) fn unavailable() -> Self {
        Self::new(
            SilverStore::unavailable(),
            qdrant_index::Store::unavailable(),
            64 * 1024 * 1024,
        )
    }

    pub(in crate::shared) fn max_node_bytes(&self) -> u64 {
        self.max_upload_bytes
    }

    /// One publication per node at a time within this process.
    async fn lock(
        &self,
        identity: &Identity,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, AppError> {
        let lock = self
            .nodes
            .lock()
            .map_err(|_| AppError::internal("shared node locks are poisoned"))?
            .entry((identity.group_id.clone(), identity.node_id.clone()))
            .or_default()
            .clone();
        Ok(lock.lock_owned().await)
    }

    /// Read one node's silver on a blocking worker.
    async fn read<T: Send + 'static>(
        &self,
        identity: &Identity,
        work: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError> + Send + 'static,
    ) -> Result<T, AppError> {
        let silver = self.silver.clone();
        let identity = identity.clone();
        crate::blocking(move || work(&silver.connect(&identity)?)).await
    }

    async fn heads(&self, group: &str) -> Result<BTreeMap<String, Head>, AppError> {
        let silver = self.silver.clone();
        let group = group.to_owned();
        crate::blocking(move || silver.heads(&group)).await
    }

    async fn head(&self, identity: &Identity) -> Result<Option<Head>, AppError> {
        self.read(identity, silver_store::head).await
    }

    pub(in crate::shared) async fn status(&self, identity: &Identity) -> Result<Counts, AppError> {
        let mut counts = self.read(identity, silver_store::counts).await?;
        counts.embeddings = self
            .gold
            .node_gold(identity)
            .await?
            .filter(|gold| gold.compatible(&self.gold, identity))
            .map_or(0, |gold| gold.embeddings());
        Ok(counts)
    }

    /// Gold usable for this node: built with the group recipe and model space.
    async fn compatible_gold(&self, identity: &Identity) -> Result<Option<NodeGold>, AppError> {
        Ok(self
            .gold
            .node_gold(identity)
            .await?
            .filter(|gold| gold.compatible(&self.gold, identity)))
    }

    /// One node's publication state, for tests.
    #[cfg(test)]
    pub(in crate::shared) async fn state(
        &self,
        identity: &Identity,
    ) -> Result<protocol::NodeState, AppError> {
        let (head, tombstones) = self
            .read(identity, |connection| {
                Ok((
                    silver_store::head(connection)?,
                    silver_store::tombstones(connection)?,
                ))
            })
            .await?;
        // A process with superseded gold settings must not steer node publication.
        self.gold.gold_configuration(identity).await?;
        Ok(protocol::NodeState {
            publication_version: head
                .as_ref()
                .map_or_else(String::new, |h| h.version.clone()),
            gold_build: self.gold.build().clone(),
            silver_revision: head.as_ref().and_then(|h| h.silver_revision.clone()),
            source_silver: head.as_ref().and_then(|h| h.source_silver.clone()),
            instance: head.as_ref().map(|h| h.instance.clone()),
            revision: head.as_ref().map_or(0, |h| h.revision),
            tombstones,
        })
    }

    pub(in crate::shared) async fn freshness(
        &self,
        identity: &Identity,
    ) -> Result<Freshness, AppError> {
        let group_gold = self.gold.gold_configuration(identity).await?;
        let heads = self.heads(&identity.group_id).await?;
        let mut nodes = Vec::new();
        for (node_id, head) in heads {
            let node = Identity {
                node_id: node_id.clone(),
                ..identity.clone()
            };
            let mut counts = self.read(&node, silver_store::counts).await?;
            let connector_inputs = self.connector_inputs(&node).await?;
            let gold = self.gold.node_gold(&node).await?;
            let silver = head.silver_revision.as_ref();
            let lexical_current = gold.as_ref().is_some_and(|g| {
                !connector_inputs && silver.is_some() && g.silver.as_ref() == silver
            });
            let gold_current = gold
                .as_ref()
                .is_some_and(|g| !connector_inputs && g.current(&self.gold, &node, silver));
            counts.embeddings = gold
                .as_ref()
                .filter(|g| g.compatible(&self.gold, &node))
                .map_or(0, NodeGold::embeddings);
            let build = self.gold.build();
            nodes.push(NodeFreshness {
                artifacts: Some(crate::gold::readiness::Artifacts {
                    metadata: crate::gold::readiness::State::publication(true, !connector_inputs),
                    lexical: crate::gold::readiness::State::publication(
                        gold.is_some(),
                        lexical_current,
                    ),
                    dense: if build.dense_enabled() {
                        crate::gold::readiness::State::publication(gold.is_some(), gold_current)
                    } else {
                        crate::gold::readiness::State::Disabled
                    },
                }),
                lexical_manifest: lexical_current
                    .then(|| silver.cloned())
                    .flatten()
                    .map(crate::shared::lexical_manifest::Manifest::new),
                lexical_current,
                gold_manifest: gold_current
                    .then(|| silver.cloned())
                    .flatten()
                    .map(|silver| {
                        crate::shared::gold_manifest::Manifest::new(
                            build.clone(),
                            node.generation.clone(),
                            silver,
                        )
                    }),
                gold_current,
                silver_revision: head.silver_revision.clone(),
                source_silver: head.source_silver.clone(),
                node_id,
                instance: head.instance.clone(),
                revision: head.revision,
                last_synced_at: head.last_synced_at.clone(),
                last_updated_at: head.last_updated_at.clone(),
                conversations: i64::try_from(counts.conversations).unwrap_or(i64::MAX),
                messages: i64::try_from(counts.messages).unwrap_or(i64::MAX),
            });
        }
        Ok(Freshness {
            group_id: identity.group_id.clone(),
            unpublished_nodes: None,
            group_gold: Some(Box::new(group_gold)),
            nodes,
        })
    }

    /// Searchable nodes. Lexical documents survive recipe and model changes;
    /// related-discussion search fails closed while any searched node's dense
    /// gold is incompatible, instead of silently omitting that node.
    async fn searchable_nodes(
        &self,
        identity: &Identity,
        dense: bool,
        exclude_self: bool,
    ) -> Result<BTreeSet<String>, AppError> {
        let heads = self.heads(&identity.group_id).await?;
        let mut active = BTreeSet::new();
        for node_id in heads.into_keys() {
            if exclude_self && node_id == identity.node_id {
                continue;
            }
            let node = Identity {
                node_id: node_id.clone(),
                ..identity.clone()
            };
            let Some(gold) = self.gold.node_gold(&node).await? else {
                continue;
            };
            if dense && !gold.searchable(&self.gold, &node) {
                return Err(AppError::shared(
                    "shared gold is stale, incomplete or incompatible; republish from nodes or restart the API with --replay-gold",
                ));
            }
            active.insert(node_id);
        }
        Ok(active)
    }

    pub(in crate::shared) async fn search(
        &self,
        identity: &Identity,
        input: protocol::Search,
    ) -> Result<protocol::Candidates, AppError> {
        qdrant_index::validate_search(&input, identity)?;
        self.gold.gold_configuration(identity).await?;
        self.require_connector_gold(identity, input.exclude_self)
            .await?;
        let exact = input.intent == crate::gold::exact::Intent::ExactMention;
        let active = self
            .searchable_nodes(identity, !exact, input.exclude_self)
            .await?;
        if active.is_empty() && exact && !input.exclude_self {
            return Err(AppError::shared(
                "shared lexical index has not been initialized; run mash index",
            ));
        }
        self.gold.search(identity, input, &active).await
    }

    pub(in crate::shared) async fn list_chats(
        &self,
        identity: &Identity,
        input: crate::chat_catalog::Request,
    ) -> Result<crate::chat_catalog::Page, AppError> {
        let mut collector = crate::chat_catalog::Collector::new(input.clone())?;
        let mut threads = Vec::new();
        for node in self.heads(&identity.group_id).await?.into_keys() {
            if !input.admits_node(&node, &identity.node_id) {
                continue;
            }
            let scope = silver_store::node_identity(&identity.group_id, &node);
            threads.extend(
                self.read(&scope, move |connection| {
                    silver_store::read::threads(connection, &node)
                })
                .await?,
            );
        }
        for thread in threads {
            let row = thread.conversation;
            collector.push(crate::chat_catalog::Chat {
                conversation_id: row.id.clone(),
                provider: row.provider,
                node_id: thread.node_id,
                metadata: crate::search_results::ChatMetadata {
                    enrichment: row.enrichment,
                    title: row.title,
                    project: row.project,
                    repository: row.repository,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                },
            });
        }
        Ok(collector.finish())
    }

    /// Fail before streaming when shared silver is unreachable.
    pub(in crate::shared) async fn available(&self, identity: &Identity) -> Result<(), AppError> {
        self.head(identity).await.map(|_| ())
    }

    pub(in crate::shared) async fn view(
        &self,
        identity: &Identity,
        input: protocol::View,
    ) -> Result<Vec<crate::storage::Message>, AppError> {
        crate::shared::validation::valid_id(&input.node_id)?;
        // Node IDs from requests address files; only configured nodes exist.
        if !self
            .silver
            .nodes(&identity.group_id)
            .contains(&input.node_id)
        {
            return Ok(Vec::new());
        }
        let node = silver_store::node_identity(&identity.group_id, &input.node_id);
        let projection = matches!(
            input.content,
            crate::storage::MessageContent::SearchProjection
        );
        self.read(&node, move |connection| {
            Ok(
                silver_store::read::view(connection, &input.id, input.context)?
                    .into_iter()
                    .map(|record| silver_store::read::message(record, projection))
                    .collect(),
            )
        })
        .await
    }
}
