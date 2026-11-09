//! Shared service storage: SQLite silver is the system of record; Qdrant views are
//! derived from it and can be rebuilt by replaying silver.
use super::Identity;
use super::configuration::{Configuration, env_name};
use crate::AppError;
use crate::shared::silver_store::{self, Head, SilverStore};
use crate::shared::{Freshness, NodeFreshness, protocol};
use crate::storage::Counts;
use crate::views::qdrant::{self as qdrant_index, NodeViews};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

mod replay;
#[cfg(test)]
pub(crate) use replay::ReplayFault;
mod records;
mod sync;

/// Serializes publications per node while other nodes proceed.
type NodeLock = Arc<tokio::sync::Mutex<()>>;

/// Runs independent per-node work concurrently. Input order is preserved and
/// the first error cancels the rest, so one slow or unavailable node cannot
/// serialize the remaining nodes.
async fn fan_out<T, U, I, F, Fut>(items: I, work: F) -> Result<Vec<U>, AppError>
where
    I: IntoIterator<Item = T>,
    F: FnMut(T) -> Fut,
    Fut: std::future::Future<Output = Result<U, AppError>>,
{
    futures_util::future::try_join_all(items.into_iter().map(work)).await
}

#[derive(Clone)]
pub(crate) struct Store {
    silver: SilverStore,
    views: qdrant_index::Store,
    max_upload_bytes: u64,
    embeddings: crate::config::Embeddings,
    nodes: Arc<std::sync::Mutex<HashMap<(String, String), NodeLock>>>,
    /// Serializes the short read-check-write that stamps node views. Distinct
    /// from `nodes`: certification may not run under the publication lock.
    views_writes: Arc<std::sync::Mutex<HashMap<(String, String), NodeLock>>>,
    /// Replays are whole-group bulk writes; their threshold is group-wide.
    replays: Arc<std::sync::Mutex<HashMap<String, NodeLock>>>,
    /// Whether HTTP read responses mask recognizable credentials.
    responses: crate::redaction::Policy,
    #[cfg(test)]
    replay_fault: Option<ReplayFault>,
    /// Test hook: fail views certification after a committed silver write.
    #[cfg(test)]
    views_interruption: bool,
    /// Test hook: fail after documents were written but before they are
    /// certified, leaving a partially written view behind.
    #[cfg(test)]
    views_partial_interruption: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
    let name = config
        .qdrant_grpc_url_env
        .as_ref()
        .ok_or_else(|| AppError::configuration("Qdrant requires qdrant_grpc_url_env"))?;
    env_name(name)?;
    let grpc_url = std::env::var(name)
        .map_err(|_| AppError::configuration(format!("set {name} to the Qdrant gRPC endpoint")))?;
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
        (&url, &grpc_url),
        key.as_deref(),
        &groups,
        384,
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
    Ok(Store::new(silver, views, config.max_upload_bytes()?)
        .with_embeddings(config.embeddings.clone()))
}

/// Bound on one Qdrant response; documents are read a page at a time.
const RESPONSE_BYTES: usize = 64 * 1024 * 1024;

impl Store {
    #[cfg(test)]
    pub(crate) fn with_views_interruption(mut self) -> Self {
        self.views_interruption = true;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_views_partial_interruption(self) -> Self {
        self.views_partial_interruption
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self
    }

    #[cfg(test)]
    pub(crate) fn clear_views_partial_interruption(&self) {
        self.views_partial_interruption
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn new(
        silver: SilverStore,
        views: qdrant_index::Store,
        max_upload_bytes: u64,
    ) -> Self {
        Self {
            silver,
            views,
            max_upload_bytes,
            embeddings: crate::config::Embeddings::Local {},
            nodes: Arc::default(),
            views_writes: Arc::default(),
            replays: Arc::default(),
            responses: crate::redaction::Policy::ON,
            #[cfg(test)]
            replay_fault: None,
            #[cfg(test)]
            views_interruption: false,
            #[cfg(test)]
            views_partial_interruption: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
        }
    }

    pub(crate) fn with_embeddings(mut self, embeddings: crate::config::Embeddings) -> Self {
        self.embeddings = embeddings;
        self
    }

    pub(crate) fn with_response_redaction(mut self, responses: crate::redaction::Policy) -> Self {
        self.responses = responses;
        self.views = self.views.with_response_redaction(responses);
        self
    }

    pub(crate) fn responses(&self) -> crate::redaction::Policy {
        self.responses
    }

    #[cfg(test)]
    pub(crate) fn unavailable() -> Self {
        Self::new(
            SilverStore::unavailable(),
            qdrant_index::Store::unavailable(),
            64 * 1024 * 1024,
        )
    }

    pub(crate) fn max_node_bytes(&self) -> u64 {
        self.max_upload_bytes
    }

    /// Serializes the views stamp check with its write, so a slow
    /// certification cannot overwrite a newer one after the guard has passed.
    pub(crate) async fn views_write_guard(
        &self,
        identity: &Identity,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, AppError> {
        let lock = self
            .views_writes
            .lock()
            .map_err(|_| AppError::internal("shared views write locks are poisoned"))?
            .entry((identity.group_id.clone(), identity.node_id.clone()))
            .or_default()
            .clone();
        Ok(lock.lock_owned().await)
    }

    /// Run a typed, read-only query over this node's shared silver. The
    /// connection gets the same views, `query_only`, authorizer and budgets as a
    /// local query; it is opened per call and discarded.
    pub(crate) async fn query(
        &self,
        identity: &Identity,
        request: crate::silver_query::Request,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, AppError> {
        self.read(identity, move |connection| {
            crate::silver_query::run_request_on(connection, &request, timeout)
        })
        .await
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

    /// One whole-group replay at a time: the bulk threshold is collection-wide.
    async fn replay_lock(
        &self,
        group_id: &str,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, AppError> {
        let lock = self
            .replays
            .lock()
            .map_err(|_| AppError::internal("shared replay locks are poisoned"))?
            .entry(group_id.to_owned())
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

    /// Stage views without certifying them. Used by tests that reconstruct a
    /// certification which lost the race with a newer commit.
    #[cfg(test)]
    pub(crate) async fn put_views(
        &self,
        identity: &Identity,
        views: &NodeViews,
    ) -> Result<(), AppError> {
        self.views.put_node_views(identity, views).await
    }

    pub(crate) async fn status(&self, identity: &Identity) -> Result<Counts, AppError> {
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
    pub(crate) async fn compatible_views(
        &self,
        identity: &Identity,
    ) -> Result<Option<NodeViews>, AppError> {
        Ok(self
            .views
            .node_views(identity)
            .await?
            .filter(|views| views.compatible(&self.views, identity)))
    }

    /// Configured nodes of one group, in identity order.
    pub(crate) fn nodes(&self, group: &str) -> &[String] {
        self.silver.nodes(group)
    }

    /// Fingerprint and message count per chat, for duplicate accounting.
    async fn fingerprinted_chats(
        &self,
        identity: &Identity,
    ) -> Result<std::collections::BTreeMap<String, (String, u64)>, AppError> {
        self.read(identity, |connection| {
            let mut counts = std::collections::BTreeMap::new();
            let mut statement = connection
                .prepare(
                    "SELECT chat_id, COUNT(*) FROM messages
                     GROUP BY chat_id",
                )
                .map_err(AppError::database)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(AppError::database)?;
            for row in rows {
                let (id, count) = row.map_err(AppError::database)?;
                counts.insert(id, u64::try_from(count).unwrap_or(u64::MAX));
            }
            drop(statement);
            let fingerprints = silver_store::fingerprints(connection)?;
            Ok(fingerprints
                .into_iter()
                .map(|(id, fingerprint)| {
                    let messages = counts.get(&id).copied().unwrap_or(0);
                    (id, (fingerprint, messages))
                })
                .collect())
        })
        .await
    }

    /// Count chats another node already holds with the same content.
    pub(crate) async fn fingerprint_report(
        &self,
        group: &str,
        dimensions: usize,
    ) -> Result<crate::shared::FingerprintReport, AppError> {
        let nodes = self.nodes(group).to_vec();
        let mut copies: std::collections::HashMap<(String, String), (u64, u64)> =
            std::collections::HashMap::new();
        let mut per_node = Vec::new();
        for node in &nodes {
            let identity = silver_store::node_identity(group, node);
            let fingerprints = self.fingerprinted_chats(&identity).await?;
            for (id, (fingerprint, messages)) in &fingerprints {
                let copy = copies
                    .entry((id.clone(), fingerprint.clone()))
                    .or_insert((0, *messages));
                copy.0 += 1;
            }
            per_node.push((node.clone(), fingerprints));
        }
        let mut summaries = Vec::new();
        for (node, fingerprints) in per_node {
            let duplicated = fingerprints
                .iter()
                .filter(|(id, (fingerprint, _))| {
                    copies
                        .get(&((*id).clone(), (*fingerprint).clone()))
                        .is_some_and(|(count, _)| *count > 1)
                })
                .count();
            summaries.push(crate::shared::NodeFingerprintSummary {
                node_id: node,
                chats: u64::try_from(fingerprints.len()).unwrap_or(u64::MAX),
                duplicated: u64::try_from(duplicated).unwrap_or(u64::MAX),
            });
        }
        let mut duplicated_chats = 0u64;
        let mut redundant_copies = 0u64;
        let mut redundant_messages = 0u64;
        for (count, messages) in copies.values().filter(|(count, _)| *count > 1) {
            duplicated_chats += 1;
            let extra = count - 1;
            redundant_copies += extra;
            redundant_messages += extra.saturating_mul(*messages);
        }
        let dimensions = u64::try_from(dimensions).unwrap_or(0);
        Ok(crate::shared::FingerprintReport {
            duplicated_chats,
            redundant_copies,
            redundant_messages,
            duplicate_vector_bytes: redundant_messages
                .saturating_mul(dimensions)
                .saturating_mul(4),
            nodes: summaries,
        })
    }

    /// Receipt recorded on the silver head for the last `/v1/sync` operation.
    #[cfg(test)]
    pub(crate) async fn sync_receipt(
        &self,
        identity: &Identity,
    ) -> Result<Option<(String, String, String)>, AppError> {
        self.read(identity, |connection| {
            Ok(silver_store::head(connection)?.and_then(|head| {
                let operation = head.operation_id?;
                Some((
                    operation,
                    head.manifest_digest.unwrap_or_default(),
                    head.receipt_digest.unwrap_or_default(),
                ))
            }))
        })
        .await
    }

    /// One node's publication state, for tests.
    #[cfg(test)]
    pub(crate) async fn state(&self, identity: &Identity) -> Result<protocol::NodeState, AppError> {
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

    pub(crate) async fn freshness(&self, identity: &Identity) -> Result<Freshness, AppError> {
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
            let silver = head.silver_revision.as_ref();
            let lexical_current = views
                .as_ref()
                .is_some_and(|g| silver.is_some() && g.silver.as_ref() == silver);
            let views_current = views
                .as_ref()
                .is_some_and(|g| g.current(&self.views, &node, silver));
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
                chats: i64::try_from(counts.chats).unwrap_or(i64::MAX),
                messages: i64::try_from(counts.messages).unwrap_or(i64::MAX),
            });
        }
        Ok(Freshness {
            baseline: None,
            activity: None,
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
        // Replay can create empty views for configured nodes that have never
        // published. Only committed publication heads establish searchable
        // membership; Qdrant metadata alone must not add a node to the group.
        let nodes = self
            .heads(&identity.group_id)
            .await?
            .into_keys()
            .filter(|node_id| !(exclude_self && node_id == &identity.node_id))
            .collect::<Vec<_>>();
        let checked = fan_out(nodes, |node_id| {
            let node = Identity {
                node_id: node_id.clone(),
                ..identity.clone()
            };
            async move {
                let Some(views) = self.views.node_views(&node).await? else {
                    return Ok(None);
                };
                if dense && !views.searchable(&self.views, &node) {
                    return Err(AppError::shared(
                        "shared views are stale, incomplete or incompatible; republish from nodes or restart the API with --replay-views",
                    ));
                }
                Ok(Some(node_id))
            }
        })
        .await?;
        Ok(checked.into_iter().flatten().collect())
    }

    pub(crate) async fn search(
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
        self.hydrate_turns(identity, &mut candidates).await?;
        Ok(candidates)
    }

    /// Derived turns live only in silver and never trigger point rewrites.
    async fn hydrate_turns(
        &self,
        identity: &Identity,
        candidates: &mut protocol::Candidates,
    ) -> Result<(), AppError> {
        let mut wanted = BTreeMap::<String, BTreeSet<_>>::new();
        for hit in candidates.lexical.iter().chain(&candidates.semantic) {
            if let Some(node) = &hit.node_id {
                wanted.entry(node.clone()).or_default().insert((
                    hit.id.clone(),
                    hit.provider,
                    hit.chat_id.clone(),
                ));
            }
        }
        let found = fan_out(wanted, |(node, messages)| async move {
            let scope = silver_store::node_identity(&identity.group_id, &node);
            let turns = self
                .read(&scope, move |connection| {
                    messages
                        .into_iter()
                        .map(|(id, provider, chat)| {
                            let turn = crate::storage::turns::message_turn(
                                connection, provider, &chat, &id,
                            )?;
                            Ok((id, turn))
                        })
                        .collect::<Result<BTreeMap<_, _>, AppError>>()
                })
                .await?;
            Ok::<_, AppError>((node, turns))
        })
        .await?
        .into_iter()
        .flat_map(|(node, turns)| {
            turns
                .into_iter()
                .map(move |(id, turn)| ((node.clone(), id), turn))
        })
        .collect::<BTreeMap<_, _>>();
        for hit in candidates
            .lexical
            .iter_mut()
            .chain(&mut candidates.semantic)
        {
            if let Some(node) = &hit.node_id {
                hit.turn_id = found
                    .get(&(node.clone(), hit.id.clone()))
                    .cloned()
                    .flatten();
            }
        }
        Ok(())
    }

    pub(crate) async fn list_chats(
        &self,
        identity: &Identity,
        input: crate::chat_catalog::Request,
    ) -> Result<crate::chat_catalog::Page, AppError> {
        let mut collector = crate::chat_catalog::Collector::new(input.clone())?;
        for chat in self.catalog(identity, &input).await? {
            collector.push(chat);
        }
        Ok(collector.finish())
    }

    pub(crate) async fn catalog(
        &self,
        identity: &Identity,
        input: &crate::chat_catalog::Request,
    ) -> Result<Vec<crate::chat_catalog::Chat>, AppError> {
        let mut threads = Vec::new();
        for node in self.heads(&identity.group_id).await?.into_keys() {
            if !input.admits_node(&node, &identity.node_id) {
                continue;
            }
            let scope = silver_store::node_identity(&identity.group_id, &node);
            threads.extend(
                self.read(&scope, move |connection| {
                    silver_store::read::list_chats(connection, &node)
                })
                .await?,
            );
        }
        let mut chats = Vec::new();
        for thread in threads {
            let row = thread.chat;
            chats.push(crate::chat_catalog::Chat {
                chat_id: row.id.clone(),
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
        Ok(chats)
    }

    pub(crate) async fn chat_page(
        &self,
        identity: &Identity,
        input: protocol::ChatPage,
    ) -> Result<Vec<crate::storage::Message>, AppError> {
        crate::shared::validation::valid_id(&input.node_id)?;
        if !self
            .silver
            .nodes(&identity.group_id)
            .contains(&input.node_id)
        {
            return Ok(Vec::new());
        }
        let node = silver_store::node_identity(&identity.group_id, &input.node_id);
        self.read(&node, move |connection| {
            silver_store::read::chat_page(connection, &input)
        })
        .await
    }

    /// Fail before streaming when shared silver is unreachable.
    pub(crate) async fn available(&self, identity: &Identity) -> Result<(), AppError> {
        self.head(identity).await.map(|_| ())
    }

    pub(crate) async fn view(
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

#[cfg(test)]
mod tests {
    use super::fan_out;
    use crate::AppError;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// Every future must run to the rendezvous point before any may proceed.
    /// Serialized execution would never reach the third arrival and time out.
    #[tokio::test]
    async fn fan_out_starts_every_node_before_any_finishes() {
        let started = Arc::new(AtomicUsize::new(0));
        let (release, _) = tokio::sync::broadcast::channel::<()>(4);
        let release = Arc::new(release);
        let work = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            move |node: u64| {
                let started = Arc::clone(&started);
                let sender = Arc::clone(&release);
                let mut receiver = sender.subscribe();
                async move {
                    if started.fetch_add(1, Ordering::SeqCst) == 2 {
                        sender.send(()).expect("broadcast receiver");
                    }
                    receiver.recv().await.expect("fan-out rendezvous");
                    Ok::<_, AppError>(node * 10)
                }
            }
        };
        let output = tokio::time::timeout(Duration::from_secs(5), fan_out([1_u64, 2, 3], work))
            .await
            .expect("a slow node must not serialize the others")
            .expect("fan-out succeeds");
        assert_eq!(output, vec![10, 20, 30], "input order is preserved");
    }

    #[tokio::test]
    async fn fan_out_fails_on_an_unavailable_node_without_losing_others() {
        let output: Result<Vec<u64>, AppError> = fan_out([1_u64, 2, 3], |node| async move {
            if node == 2 {
                return Err(AppError::shared("node two is unavailable"));
            }
            Ok(node)
        })
        .await;
        let error = output.expect_err("an unavailable node must fail the group");
        assert!(error.error.message.contains("node two"));
    }
}
