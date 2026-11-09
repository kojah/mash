//! Direct local Qdrant projection from committed SQLite silver. No HTTP service.
use super::{
    bulk::{BulkPolicy, BulkViews},
    manifest::Manifest,
    qdrant::{NodeViews, ProjectionEvidence, Store},
    scope::Scope,
};
use crate::{
    AppError,
    config::ResolvedConfig,
    semantic,
    storage::{Counts, Storage, gold_input::Input},
};
use std::{path::Path, sync::Arc, time::Instant};

mod projection;

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub(crate) struct Build {
    pub(crate) counts: Counts,
    pub(crate) ready: bool,
    pub(crate) manifest: Option<Manifest>,
    pub(crate) embeddings: semantic::EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) search_refresh_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

pub(crate) struct Local {
    database: std::path::PathBuf,
    instance: String,
    pub(crate) store: Store,
    pub(crate) scope: Scope,
    pub(crate) certificate: crate::silver::certificate::Certificate,
}

fn key(configuration: &ResolvedConfig) -> Result<Option<String>, AppError> {
    configuration
        .qdrant
        .api_key_env
        .as_ref()
        .map(|name| {
            std::env::var(name).map_err(|_| {
                AppError::configuration(format!("set {name} to the local Qdrant API key"))
            })
        })
        .transpose()
}

fn scope(instance: &str, configuration: &ResolvedConfig) -> Scope {
    let node = configuration
        .shared
        .as_ref()
        .map_or("local", |shared| shared.node_id.as_str());
    let node_key = blake3::hash(node.as_bytes()).to_hex();
    Scope {
        group_id: format!("local-{instance}-{}", &node_key[..16]),
        node_id: node.into(),
        generation: semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
        dimensions: 384,
    }
}

fn revision(
    certificate: &crate::silver::certificate::Certificate,
) -> Result<crate::silver::revision::State, AppError> {
    let bytes = serde_json::to_vec(certificate)
        .map_err(|_| AppError::internal("encode local silver identity"))?;
    crate::silver::revision::State::next(None, blake3::hash(&bytes).to_hex().to_string())
}

impl Local {
    /// Readers verify existing collections and settings without creating them.
    pub(crate) async fn open(
        database: &Path,
        configuration: &ResolvedConfig,
    ) -> Result<Self, AppError> {
        Self::open_inner(database, configuration, false).await
    }

    pub(crate) async fn open_exact(
        database: &Path,
        configuration: &ResolvedConfig,
    ) -> Result<Self, AppError> {
        Self::open_inner(database, configuration, true).await
    }

    async fn open_inner(
        database: &Path,
        configuration: &ResolvedConfig,
        exact: bool,
    ) -> Result<Self, AppError> {
        let source = database.to_path_buf();
        let database = source.clone();
        let (instance, certificate) =
            crate::blocking(move || Storage::open_silver(&database)?.local_gold_state(&database))
                .await?;
        let scope = scope(&instance, configuration);
        let key = key(configuration)?;
        let mut store = Store::open(
            &configuration.qdrant.url,
            key.as_deref(),
            std::slice::from_ref(&scope.group_id),
            scope.dimensions,
            64 * 1024 * 1024,
            configuration.qdrant.vector_search,
            configuration.qdrant.memory,
        )
        .await?
        .with_build(configuration.views.build.clone())
        .with_response_redaction(configuration.redaction.output());
        if exact {
            store = store.with_lexical_build(&scope).await?;
        }
        store.views_configuration(&scope).await?;
        Ok(Self {
            database: source,
            instance,
            store,
            scope,
            certificate,
        })
    }

    pub(crate) async fn current(&self) -> Result<Option<NodeViews>, AppError> {
        let database = self.database.clone();
        let (instance, current) =
            crate::blocking(move || Storage::open_silver(&database)?.local_gold_state(&database))
                .await?;
        if instance != self.instance || current != self.certificate {
            return Ok(None);
        }
        let state = revision(&self.certificate)?;
        Ok(self.store.node_views(&self.scope).await?.filter(|node| {
            self.certificate.permits_views() && node.current(&self.store, &self.scope, Some(&state))
        }))
    }

    pub(crate) async fn candidates(
        &self,
        input: super::retrieval::Search,
    ) -> Result<super::retrieval::Candidates, AppError> {
        use crate::observability::retrieval::{Guard, Stage};
        let mut measured = Guard::begin(if input.intent == super::exact::Intent::ExactMention {
            Stage::LocalExact
        } else {
            Stage::LocalCandidates
        });
        if self.current().await?.is_none() {
            return Err(AppError::search_not_ready(
                "local gold is not current; run mash index",
            ));
        }
        let active = std::collections::BTreeSet::from([self.scope.node_id.clone()]);
        let mut result = self.store.search(&self.scope, input, &active).await?;
        let database = self.database.clone();
        result = crate::blocking(move || {
            Storage::open_silver(&database)?.hydrate_turns(&mut result)?;
            Ok(result)
        })
        .await?;
        if self.current().await?.is_none() {
            return Err(AppError::search_not_ready(
                "local silver changed during retrieval; retry after indexing",
            ));
        }
        measured.complete(None, Some(result.lexical.len() + result.semantic.len()));
        Ok(result)
    }
}

/// Caller retains local writer admission from ingestion through certification.
#[allow(
    clippy::too_many_lines,
    reason = "keep invalidation, awaited mutations and final certification in one coordinator"
)]
pub(crate) async fn build(
    database: &Path,
    models: &Path,
    configuration: &ResolvedConfig,
    full: bool,
) -> Result<Build, AppError> {
    let started = Instant::now();
    crate::progress::emit(&crate::progress::Event::new(
        crate::progress::Phase::SearchRebuild,
        0,
        None,
    ));
    let path = database.to_path_buf();
    let input = Arc::new(crate::blocking(move || Input::capture(&path)).await?);
    let scope = scope(&input.instance, configuration);
    let key = key(configuration)?;
    let store = Store::connect(
        (&configuration.qdrant.url, &configuration.qdrant.grpc_url),
        key.as_deref(),
        std::slice::from_ref(&scope.group_id),
        scope.dimensions,
        64 * 1024 * 1024,
        configuration.qdrant.vector_search,
        super::qdrant::MemoryPolicy {
            tier: configuration.qdrant.memory,
            activate: true,
        },
    )
    .await?
    .with_build(configuration.views.build.clone());
    let version = store.select_views(&scope, true).await?;
    store.select_query(&scope, true).await?;
    let bulk = BulkViews::begin(&store, &scope, BulkPolicy::local()).await?;
    let previous = store
        .node_views(&scope)
        .await?
        .filter(|node| node.compatible(&store, &scope) && node.certified && !full);
    let state = revision(&input.certificate)?;
    if input.certificate.permits_views()
        && let Some(node) = &previous
        && node.current(&store, &scope, Some(&state))
    {
        return Ok(unchanged(&input, &scope, configuration, node, started));
    }
    let (certificate, node, work) = bulk
        .run(
            || async {
                let mut node = NodeViews::empty(&store, &scope);
                // Persist stale metadata before any document mutation,
                // including deletions.
                bulk.invalidate(&node).await?;
                bulk.retain_chats(input.chats.keys().cloned()).await?;
                let mut writer = bulk
                    .writer(models)
                    .with_execution(configuration.embeddings.clone());
                Box::pin(projection::native(
                    &store,
                    &scope,
                    &mut writer,
                    &input,
                    previous.as_ref(),
                    &mut node,
                ))
                .await?;
                let work = bulk.finish(writer).await?;
                let path = database.to_path_buf();
                let certificate =
                    crate::blocking(move || Storage::open_silver(&path)?.silver_certificate())
                        .await?;
                if certificate != input.certificate
                    || store.views_configuration(&scope).await?.version != version
                {
                    return Err(AppError::search_not_ready(
                        "local silver or views settings changed during refresh; retry indexing",
                    ));
                }
                node.input = Some(
                    i64::try_from(certificate.revision)
                        .map_err(|_| AppError::database_data("silver revision overflow"))?,
                );
                let ready = certificate.permits_views() && node.projections.complete();
                node.certified = ready;
                node.silver = ready.then_some(state);
                // The single completion barrier: strongly ordered after the
                // stale state, the delete and every unwaited document batch.
                store.put_node_views(&scope, &node).await?;
                Ok((certificate, node, work))
            },
            |(_, node, work)| emit_progress(node.stored_vectors(), &work.embeddings, started),
        )
        .await?;
    let stored = node.stored_vectors();
    let ready = node.certified;
    let mut embeddings = work.embeddings;
    embeddings.stored_vectors = stored;
    embeddings.reused_vectors = stored.saturating_sub(embeddings.model_inferences);
    Ok(Build {
        counts: input.counts.clone(),
        ready,
        manifest: ready.then(|| {
            Manifest::new(
                configuration.views.build.clone(),
                scope.generation.clone(),
                certificate,
            )
        }),
        embeddings,
        model_load_milliseconds: work.model_load_milliseconds,
        embedding_milliseconds: work.embedding_milliseconds,
        search_refresh_milliseconds: crate::progress::milliseconds(started),
    })
}

fn unchanged(
    input: &Input,
    scope: &Scope,
    configuration: &ResolvedConfig,
    node: &NodeViews,
    started: Instant,
) -> Build {
    let stored = node.stored_vectors();
    Build {
        counts: input.counts.clone(),
        ready: true,
        manifest: Some(Manifest::new(
            configuration.views.build.clone(),
            scope.generation.clone(),
            input.certificate.clone(),
        )),
        embeddings: semantic::EmbeddingSummary {
            stored_vectors: stored,
            reused_vectors: stored,
            ..Default::default()
        },
        model_load_milliseconds: 0,
        embedding_milliseconds: 0,
        search_refresh_milliseconds: crate::progress::milliseconds(started),
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "throughput is approximate; vector counters retain their exact integers"
)]
fn emit_progress(stored: u64, embeddings: &semantic::EmbeddingSummary, started: Instant) {
    crate::progress::emit(&crate::progress::Event::embedding(
        &semantic::EmbeddingProgress {
            stored_vectors: stored,
            total_vectors: stored,
            model_inferences: embeddings.model_inferences,
            reused_vectors: stored.saturating_sub(embeddings.model_inferences),
            elapsed_milliseconds: crate::progress::milliseconds(started),
            stored_vectors_per_second: stored as f64
                / started.elapsed().as_secs_f64().max(f64::EPSILON),
            eta_seconds: Some(0),
        },
    ));
}
