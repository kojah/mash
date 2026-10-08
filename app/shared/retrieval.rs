//! Concurrent local/shared candidate retrieval, fusion and contextual ranking.
use super::client::{Endpoint, get, post};
use super::protocol;
use crate::gold::config::{Fusion, Options};
use crate::semantic::{self, Models};
use crate::storage::{SearchHit, Storage};
use crate::{AppError, config::SharedConfig};
use futures_util::FutureExt;
use std::{collections::HashMap, path::Path, time::Duration};

#[cfg(test)]
mod tests;

pub(super) const CONTEXT_DEADLINE: Duration = Duration::from_secs(1);

pub(crate) enum SharedRetrieval {
    Available,
    LocalFallback { error: String },
}

impl SharedRetrieval {
    pub(crate) fn into_error(self) -> Option<String> {
        match self {
            Self::Available => None,
            Self::LocalFallback { error } => Some(error),
        }
    }
}

pub(crate) struct SearchOutcome {
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) retrieval: SharedRetrieval,
    pub(crate) context_error: Option<String>,
}

pub(crate) async fn require_dense(config: &SharedConfig) -> Result<(), AppError> {
    let freshness: super::Freshness = get(config, Endpoint::Freshness).await?;
    freshness
        .group_gold
        .ok_or_else(|| AppError::shared("shared group gold settings are unavailable"))?
        .build
        .require_intent(crate::gold::exact::Intent::RelatedDiscussion)
}

#[tracing::instrument(name = "combined_search", skip_all)]
pub(crate) async fn search(
    config: &SharedConfig,
    local_database: Option<(&Path, &crate::gold::config::Build)>,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    query: &str,
    options: Options,
    filters: &crate::search_filter::Scope,
) -> Result<SearchOutcome, AppError> {
    filters.filter.validate()?;
    crate::search_query::validate(query)?;
    let prepared = prepare_search(
        local_database,
        models_dir,
        embeddings,
        query,
        options.ranking,
    )
    .await?;
    search_prepared(
        config,
        local_database.map(|(path, _)| path),
        query,
        options,
        filters,
        prepared,
    )
    .await
}

pub(super) struct PreparedSearch {
    backend: semantic::LoadedBackend,
    vector: Vec<f64>,
    local_generation: String,
    generation: String,
}

#[tracing::instrument(name = "prepare_search", skip_all)]
pub(super) async fn prepare_search(
    local_database: Option<(&Path, &crate::gold::config::Build)>,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    query: &str,
    ranking: crate::gold::config::Ranking,
) -> Result<PreparedSearch, AppError> {
    let root = models_dir.to_path_buf();
    let text = query.to_owned();
    let embeddings = embeddings.cloned();
    let readiness_database =
        local_database.map(|(path, build)| (path.to_path_buf(), build.clone()));
    let generation = embeddings.as_ref().map_or_else(
        || semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
        |c| semantic::model_space(&c.model, c.dimensions),
    );
    crate::blocking(move || {
        let models = Models::new(root).with_remote(embeddings);
        models.require_search(ranking)?;
        let local_build = readiness_database
            .as_ref()
            .map(|(_, build)| build)
            .cloned()
            .unwrap_or_default();
        if let Some((database, build)) = readiness_database {
            Storage::open_search(&database, &build.generation(&models.generation()))?
                .require_gold_build(&build, &models.generation())?;
        }
        let mut backend = models
            .load_search(ranking)?
            .ok_or_else(|| AppError::model("run `mash models install`"))?;
        let vector = semantic::shared_query_vector(&mut backend, &text)?;
        Ok(PreparedSearch {
            backend,
            vector,
            local_generation: local_build.generation(&models.generation()),
            generation,
        })
    })
    .await
}

#[tracing::instrument(name = "retrieve_and_rank", skip_all)]
pub(super) async fn search_prepared(
    config: &SharedConfig,
    local_database: Option<&Path>,
    query: &str,
    options: Options,
    filters: &crate::search_filter::Scope,
    prepared: PreparedSearch,
) -> Result<SearchOutcome, AppError> {
    let PreparedSearch {
        mut backend,
        vector,
        local_generation,
        generation,
    } = prepared;
    let Options {
        limit,
        unit,
        fusion,
        ranking,
        shared_vector_search,
    } = options;
    let local_filters = filters.clone();
    let local_database = local_database.map(Path::to_path_buf);
    let context_database = local_database.clone();
    let context_generation = local_generation.clone();
    let exclude_self = local_database.is_some();
    let local_query = query.to_owned();
    let local_vector = vector.clone();
    let local = crate::blocking(move || {
        retrieve_local_candidates(
            local_database.as_deref(),
            &local_generation,
            &local_query,
            &local_vector,
            &local_filters,
            unit,
            fusion,
        )
    });
    let request = protocol::Search {
        match_mode: filters.match_mode,
        vector_policy: shared_vector_search,
        intent: crate::gold::exact::Intent::RelatedDiscussion,
        query: query.to_owned(),
        unit,
        vector,
        generation,
        provider: None,
        days: None,
        filter: Some(filters.filter.clone()),
        exclude_self,
    };
    // Keep HTTP errors as values until local retrieval finishes; a local error
    // still cancels HTTP, while merged searches may fall back locally.
    let remote = request_candidates(config, &request).map(Ok::<_, AppError>);
    let ((local_hits, local_documents), candidates) = tokio::try_join!(local, remote)?;
    let (candidates, retrieval) = received_candidates(candidates, exclude_self)?;
    let protocol::Candidates {
        mut lexical,
        mut semantic,
        documents,
    } = candidates;
    let mut documents = projected_documents(documents);
    for (hit, projection) in local_hits.iter().zip(local_documents) {
        documents.insert((String::new(), hit.id.clone()), projection);
    }
    // Older servers returned canonical history in hits. Always search and
    // display their projection, which also omits embedded tool results.
    apply_projections(lexical.iter_mut().chain(&mut semantic), &documents);
    let query = query.to_owned();
    let ranking_query = query.clone();
    let (mut backend, selection) = crate::blocking(move || {
        let fused = combine_candidates(lexical, semantic, local_hits, options);
        let selection = if ranking == crate::gold::config::Ranking::Fusion {
            crate::search_ranking::Selection {
                hits: fused,
                count: 0,
            }
        } else {
            crate::search_ranking::select(&mut backend, &ranking_query, fused, unit, limit)?
        };
        Ok((backend, selection))
    })
    .await?;
    if ranking == crate::gold::config::Ranking::Fusion {
        return finish_fused(backend, selection.hits, retrieval, options).await;
    }
    let (context, context_error) = measured_context_documents(
        config,
        context_database,
        context_generation,
        &query,
        &selection.hits[..selection.count],
    )
    .await?;
    documents.extend(context);
    crate::blocking(move || {
        let fused = rerank_combined(&mut backend, &query, selection, &documents, (limit, unit))?;
        Ok(SearchOutcome {
            hits: fused,
            retrieval,
            context_error,
        })
    })
    .await
}

fn combine_candidates(
    lexical: Vec<SearchHit>,
    semantic: Vec<SearchHit>,
    local: Vec<SearchHit>,
    options: Options,
) -> Vec<SearchHit> {
    let mut fused = semantic::fuse(lexical, semantic, options.unit, options.fusion);
    fused.extend(local);
    fused.sort_by(|a, b| {
        b.fusion_score
            .total_cmp(&a.fusion_score)
            .then_with(|| a.node_id.cmp(&b.node_id))
            .then_with(|| a.id.cmp(&b.id))
    });
    crate::search_results::diversify(fused, options.unit)
}

async fn finish_fused(
    backend: semantic::LoadedBackend,
    hits: Vec<SearchHit>,
    retrieval: SharedRetrieval,
    options: Options,
) -> Result<SearchOutcome, AppError> {
    crate::blocking(move || {
        // Returning a loaded backend to the cache can destroy the old model.
        drop(backend);
        Ok(SearchOutcome {
            hits: crate::search_ranking::finish_fusion(hits, options.unit, options.limit),
            retrieval,
            context_error: None,
        })
    })
    .await
}

async fn measured_context_documents(
    config: &SharedConfig,
    database: Option<std::path::PathBuf>,
    generation: String,
    query: &str,
    hits: &[SearchHit],
) -> Result<(HashMap<(String, String), String>, Option<String>), AppError> {
    let mut stage = crate::observability::retrieval::Guard::begin(
        crate::observability::retrieval::Stage::Context,
    );
    let result = context_documents(config, database, generation, query, hits).await?;
    if result.1.is_none() {
        stage.complete(Some(hits.len()), Some(result.0.len()));
    }
    Ok(result)
}

async fn request_candidates(
    config: &SharedConfig,
    input: &protocol::Search,
) -> Result<protocol::Candidates, AppError> {
    let mut measured = crate::observability::retrieval::Guard::begin(
        crate::observability::retrieval::Stage::SharedCandidates,
    );
    let candidates = post::<protocol::Candidates>(config, Endpoint::Search, input).await?;
    measured.complete(
        None,
        Some(
            candidates
                .lexical
                .len()
                .saturating_add(candidates.semantic.len()),
        ),
    );
    Ok(candidates)
}

fn received_candidates(
    candidates: Result<protocol::Candidates, AppError>,
    merged: bool,
) -> Result<(protocol::Candidates, SharedRetrieval), AppError> {
    match candidates {
        Ok(candidates) => Ok((candidates, SharedRetrieval::Available)),
        Err(error) if merged => Ok((
            protocol::Candidates {
                lexical: Vec::new(),
                semantic: Vec::new(),
                documents: Vec::new(),
            },
            SharedRetrieval::LocalFallback {
                error: error.error.message,
            },
        )),
        Err(error) => Err(error),
    }
}

fn projected_documents(documents: Vec<protocol::Document>) -> HashMap<(String, String), String> {
    documents
        .into_iter()
        .map(|d| {
            (
                (d.node_id, d.id),
                crate::silver::dialogue::visible(&crate::role::Role::User, &d.projection)
                    .into_owned(),
            )
        })
        .collect()
}

fn apply_projections<'a>(
    hits: impl Iterator<Item = &'a mut SearchHit>,
    documents: &HashMap<(String, String), String>,
) {
    for hit in hits {
        if let Some(projection) =
            documents.get(&(hit.node_id.clone().unwrap_or_default(), hit.id.clone()))
        {
            hit.content.clone_from(projection);
        }
    }
}

pub(super) async fn context_documents(
    config: &SharedConfig,
    database: Option<std::path::PathBuf>,
    generation: String,
    query: &str,
    hits: &[SearchHit],
) -> Result<(HashMap<(String, String), String>, Option<String>), AppError> {
    let local_ids = hits
        .iter()
        .filter(|hit| hit.node_id.is_none())
        .map(|hit| hit.id.clone())
        .collect::<Vec<_>>();
    let local = crate::blocking(move || {
        let mut contexts = HashMap::new();
        if let Some(database) = database.filter(|_| !local_ids.is_empty()) {
            let storage = Storage::open_search(&database, &generation)?;
            for id in local_ids {
                contexts.insert(
                    (String::new(), id.clone()),
                    storage.view_content(
                        &id,
                        2,
                        crate::storage::MessageContent::SearchProjection,
                    )?,
                );
            }
        }
        Ok(contexts)
    });
    let anchors = hits
        .iter()
        .filter_map(|hit| hit.node_id.clone().map(|node| (node, hit.id.clone())))
        .collect::<Vec<_>>();
    let remote = super::client::context_batch(config, anchors);
    let (contexts, (remote, mut error)) = tokio::join!(local, remote);
    let mut contexts = contexts?;
    for (key, result) in remote {
        match result {
            Ok(messages) => {
                contexts.insert(key, messages);
            }
            Err(failure) => {
                error.get_or_insert(failure.error.message);
            }
        }
    }
    let documents = hits
        .iter()
        .filter_map(|hit| {
            let key = (hit.node_id.clone().unwrap_or_default(), hit.id.clone());
            contexts.get(&key).map(|messages| {
                (
                    key,
                    crate::search_results::contextual_document(hit, query, messages),
                )
            })
        })
        .collect();
    Ok((documents, error))
}

#[tracing::instrument(name = "local_candidates", skip_all)]
fn retrieve_local_candidates(
    database: Option<&Path>,
    generation: &str,
    query: &str,
    vector: &[f64],
    filters: &crate::search_filter::Scope,
    unit: crate::search_results::SearchUnit,
    fusion: Fusion,
) -> Result<(Vec<SearchHit>, Vec<String>), AppError> {
    let Some(database) = database else {
        return Ok((Vec::new(), Vec::new()));
    };
    let storage = Storage::open_search(database, generation)?;
    semantic::local_candidates(
        &storage,
        generation,
        query,
        vector,
        (None, None, unit),
        Some(filters),
        fusion,
    )
}

#[tracing::instrument(name = "rerank", skip_all)]
fn rerank_combined(
    backend: &mut semantic::Backend,
    query: &str,
    selection: crate::search_ranking::Selection,
    documents: &HashMap<(String, String), String>,
    options: (usize, crate::search_results::SearchUnit),
) -> Result<Vec<SearchHit>, AppError> {
    let (limit, unit) = options;
    let selected = selection.hits[..selection.count]
        .iter()
        .map(|hit| {
            documents
                .get(&(hit.node_id.clone().unwrap_or_default(), hit.id.clone()))
                .cloned()
                .ok_or_else(|| AppError::shared("shared candidate has no search projection"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::search_ranking::score(backend, query, selection, &selected, unit, limit)
}
