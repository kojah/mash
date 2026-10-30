//! Shared service storage: SQLite silver is the system of record; Qdrant views are
//! derived from it and can be rebuilt by replaying silver.
use super::Identity;
use super::configuration::{Configuration, env_name};
use crate::AppError;
use crate::shared::qdrant_index::{self, NodeViews};
use crate::shared::silver_store::{self, Head, SilverStore};
use crate::shared::{Freshness, NodeFreshness, protocol};
use crate::storage::Counts;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

pub(super) mod connectors;
mod replay;
#[cfg(test)]
pub(in crate::shared) use replay::ReplayFault;
mod sync;

/// Serializes publications per node while other nodes proceed.
type NodeLock = Arc<tokio::sync::Mutex<()>>;

#[derive(Clone)]
pub(in crate::shared) struct Store {
    silver: SilverStore,
    views: qdrant_index::Store,
    /// Embeds connector records; absent when the server has no model setup.
    embedder: Option<connectors::Embedder>,
    max_upload_bytes: u64,
    nodes: Arc<std::sync::Mutex<HashMap<(String, String), NodeLock>>>,
    /// Whether HTTP read responses mask recognizable credentials.
    responses: crate::redaction::Policy,
    #[cfg(test)]
    replay_fault: Option<ReplayFault>,
}

pub(super) async fn initialize(
    config: &Configuration,
    identities: &HashMap<blake3::Hash, Identity>,
    activate_views: bool,
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
    let views = qdrant_index::Store::connect(
        &url,
        key.as_deref(),
        &groups,
        config.embeddings.dimensions,
        RESPONSE_BYTES,
        config.views.query.vector_search,
        qdrant_index::MemoryPolicy {
            tier: config.qdrant_memory,
            activate: activate_memory,
        },
    )
    .await?
    .with_build(config.views.build.clone());
    let mut selected = HashSet::new();
    for identity in identities.values() {
        if selected.insert(&identity.group_id) {
            let group_store = views.clone().with_query(
                config
                    .views
                    .query
                    .groups
                    .get(&identity.group_id)
                    .copied()
                    .unwrap_or(config.views.query.vector_search),
            );
            group_store.select_views(identity, activate_views).await?;
            group_store.select_query(identity, activate_query).await?;
        }
    }
    Ok(Store::new(silver, views, config.max_upload_bytes()?))
}

/// Bound on one Qdrant response; documents are read a page at a time.
const RESPONSE_BYTES: usize = 64 * 1024 * 1024;

impl Store {
    pub(in crate::shared) fn new(
        silver: SilverStore,
        views: qdrant_index::Store,
        max_upload_bytes: u64,
    ) -> Self {
        Self {
            silver,
            views,
            embedder: None,
            max_upload_bytes,
            nodes: Arc::default(),
            responses: crate::redaction::Policy::ON,
            #[cfg(test)]
            replay_fault: None,
        }
    }

    pub(in crate::shared) fn with_response_redaction(
        mut self,
        responses: crate::redaction::Policy,
    ) -> Self {
        self.responses = responses;
        self.views = self.views.with_response_redaction(responses);
        self
    }

    pub(in crate::shared) fn responses(&self) -> crate::redaction::Policy {
        self.responses
    }

    pub(in crate::shared) fn with_embedder(mut self, embedder: connectors::Embedder) -> Self {
        self.embedder = Some(embedder);
        self
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
            .views
            .node_views(identity)
            .await?
            .filter(|views| views.compatible(&self.views, identity))
            .map_or(0, |views| views.embeddings());
        Ok(counts)
    }

    /// View usable for this node: built with the group recipe and model space.
    async fn compatible_views(&self, identity: &Identity) -> Result<Option<NodeViews>, AppError> {
        Ok(self
            .views
            .node_views(identity)
            .await?
            .filter(|views| views.compatible(&self.views, identity)))
    }

    /// One node's views state, for tests.
    #[cfg(test)]
    pub(in crate::shared) async fn views_state(
        &self,
        identity: &Identity,
    ) -> Result<Option<NodeViews>, AppError> {
        self.views.node_views(identity).await
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
        // A process with superseded views settings must not steer node publication.
        self.views.views_configuration(identity).await?;
        Ok(protocol::NodeState {
            publication_version: head
                .as_ref()
                .map_or_else(String::new, |h| h.version.clone()),
            views_build: self.views.build().clone(),
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
        let group_views = self.views.views_configuration(identity).await?;
        let heads = self.heads(&identity.group_id).await?;
        let mut nodes = Vec::new();
        for (node_id, head) in heads {
            let node = Identity {
                node_id: node_id.clone(),
                ..identity.clone()
            };
            let mut counts = self.read(&node, silver_store::counts).await?;
            let views = self.views.node_views(&node).await?;
            let connectors_current = match &views {
                Some(g) => g.connectors == self.connector_digests(&node).await?,
                None => false,
            };
            let silver = head.silver_revision.as_ref();
            let lexical_current = views.as_ref().is_some_and(|g| {
                connectors_current && silver.is_some() && g.silver.as_ref() == silver
            });
            let views_current = views
                .as_ref()
                .is_some_and(|g| connectors_current && g.current(&self.views, &node, silver));
            counts.embeddings = views
                .as_ref()
                .filter(|g| g.compatible(&self.views, &node))
                .map_or(0, NodeViews::embeddings);
            let build = self.views.build();
            nodes.push(NodeFreshness {
                artifacts: Some(crate::views::readiness::Artifacts {
                    metadata: crate::views::readiness::State::publication(true, true),
                    lexical: crate::views::readiness::State::publication(
                        views.is_some(),
                        lexical_current,
                    ),
                    dense: if build.dense_enabled() {
                        crate::views::readiness::State::publication(views.is_some(), views_current)
                    } else {
                        crate::views::readiness::State::Disabled
                    },
                }),
                lexical_manifest: lexical_current
                    .then(|| silver.cloned())
                    .flatten()
                    .map(crate::shared::lexical_manifest::Manifest::new),
                lexical_current,
                views_manifest: views_current
                    .then(|| silver.cloned())
                    .flatten()
                    .map(|silver| {
                        crate::shared::views_manifest::Manifest::new(
                            build.clone(),
                            node.generation.clone(),
                            silver,
                        )
                    }),
                views_current,
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
            group_views: Some(Box::new(group_views)),
            nodes,
        })
    }

    /// Searchable nodes. Lexical documents survive recipe and model changes;
    /// related-discussion search fails closed while any searched node's dense
    /// views are incompatible, instead of silently omitting that node.
    async fn searchable_nodes(
        &self,
        identity: &Identity,
        dense: bool,
        exclude_self: bool,
    ) -> Result<BTreeSet<String>, AppError> {
        // View exists only after a node uploads or receives connector records,
        // so configured nodes without it are simply absent from search.
        let mut active = BTreeSet::new();
        for node_id in self.silver.nodes(&identity.group_id).iter().cloned() {
            if exclude_self && node_id == identity.node_id {
                continue;
            }
            let node = Identity {
                node_id: node_id.clone(),
                ..identity.clone()
            };
            let Some(views) = self.views.node_views(&node).await? else {
                continue;
            };
            if dense && !views.searchable(&self.views, &node) {
                return Err(AppError::shared(
                    "shared views are stale, incomplete or incompatible; republish from nodes or restart the API with --replay-views",
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
        self.views.views_configuration(identity).await?;
        let exact = input.intent == crate::views::exact::Intent::ExactMention;
        let active = self
            .searchable_nodes(identity, !exact, input.exclude_self)
            .await?;
        if active.is_empty() && exact && !input.exclude_self {
            return Err(AppError::shared(
                "shared lexical index has not been initialized; run mash index",
            ));
        }
        let mut candidates = self.views.search(identity, input, &active).await?;
        self.native_ancestry(identity, &mut candidates).await?;
        Ok(candidates)
    }

    /// Native points name only their chat; each node's shared silver holds the
    /// derived turns between, so ancestry is read from it at query time. View
    /// never stores the hierarchy, and turn changes never rewrite points.
    async fn native_ancestry(
        &self,
        identity: &Identity,
        candidates: &mut protocol::Candidates,
    ) -> Result<(), AppError> {
        let mut wanted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for hit in candidates.lexical.iter().chain(&candidates.semantic) {
            if let (crate::source::Source::Provider(_), Some(node)) = (&hit.source, &hit.node_id) {
                wanted
                    .entry(node.clone())
                    .or_default()
                    .insert(hit.id.clone());
            }
        }
        let mut found = BTreeMap::new();
        for (node, ids) in wanted {
            let scope = silver_store::node_identity(&identity.group_id, &node);
            let ancestry = self
                .read(&scope, move |connection| {
                    let mut ancestry = BTreeMap::new();
                    for id in ids {
                        // A point can briefly outlive its silver row; keep its chat.
                        let Some(row) = crate::storage::generated::NativeEntryParent::builder()
                            .id(&id)
                            .build()
                            .query_opt(connection)
                            .map_err(AppError::database)?
                        else {
                            continue;
                        };
                        let ancestors =
                            crate::storage::objects::ancestors(connection, row.parent_storage_id)?;
                        ancestry.insert(id, ancestors);
                    }
                    Ok(ancestry)
                })
                .await?;
            for (id, ancestors) in ancestry {
                found.insert((node.clone(), id), ancestors);
            }
        }
        for hit in candidates
            .lexical
            .iter_mut()
            .chain(candidates.semantic.iter_mut())
        {
            if let Some(node) = &hit.node_id
                && let Some(ancestors) = found.get(&(node.clone(), hit.id.clone()))
            {
                hit.ancestors.clone_from(ancestors);
            }
        }
        Ok(())
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
        for node in self.silver.nodes(&identity.group_id) {
            if !input.admits_node(node, &identity.node_id) {
                continue;
            }
            let node = node.clone();
            let scope = silver_store::node_identity(&identity.group_id, &node);
            let chats = self
                .read(&scope, move |connection| {
                    crate::storage::connector_views::chats(connection, &node)
                })
                .await?;
            for chat in chats {
                collector.push(chat);
            }
        }
        for thread in threads {
            let row = thread.conversation;
            collector.push(crate::chat_catalog::Chat {
                conversation_id: row.id.clone(),
                source: crate::source::Source::Provider(row.provider),
                object: crate::source::ObjectType::native_chat(row.provider),
                ancestors: Vec::new(),
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
        if input
            .id
            .starts_with(crate::storage::connector_views::ANCHOR_PREFIX)
        {
            return self
                .read(&node, move |connection| {
                    crate::storage::connector_views::view(connection, &input.id, input.context)
                })
                .await;
        }
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
