//! Concurrent local/shared candidate retrieval, fusion and contextual ranking.
use super::client::{Endpoint, get, post};
use super::protocol;
use crate::semantic::{self, Models};
use crate::storage::{SearchHit, Storage};
use crate::views::config::Options;
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
        .group_views
        .ok_or_else(|| AppError::shared("shared group views settings are unavailable"))?
        .build
        .require_intent(crate::views::exact::Intent::RelatedDiscussion)
}

#[tracing::instrument(name = "combined_search", skip_all)]
pub(crate) async fn search(
    config: &SharedConfig,
    local_database: Option<(&Path, &crate::config::ResolvedConfig)>,
    models_dir: &Path,
    query: &str,
    options: Options,
    filters: &crate::search_filter::Scope,
) -> Result<SearchOutcome, AppError> {
    filters.filter.validate()?;
    crate::search_query::validate(query)?;
    let prepared = prepare_search(local_database, models_dir, query, options.ranking)
        .boxed()
        .await?;
    search_prepared(
        config,
        local_database.map(|(path, _)| path),
        query,
        options,
        filters,
        prepared,
    )
    .boxed()
    .await
}

pub(super) struct PreparedSearch {
    backend: semantic::LoadedBackend,
    vector: Vec<f64>,
    local: Option<crate::views::local::Local>,
    generation: String,
}

#[tracing::instrument(name = "prepare_search", skip_all)]
pub(super) async fn prepare_search(
    local_database: Option<(&Path, &crate::config::ResolvedConfig)>,
    models_dir: &Path,
    query: &str,
    ranking: crate::views::config::Ranking,
) -> Result<PreparedSearch, AppError> {
    let root = models_dir.to_path_buf();
    let text = query.to_owned();
    let local = match local_database {
        Some((path, configuration)) => {
            Some(crate::views::local::Local::open_current(path, configuration).await?)
        }
        None => None,
    };
    let generation = semantic::SHARED_EMBEDDING_GENERATION.to_owned();
    crate::blocking(move || {
        let models = Models::new(root);
        models.require_installed()?;
        let mut backend = models
            .load_search(ranking)?
            .ok_or_else(|| AppError::model("run `mash models install`"))?;
        let vector = semantic::shared_query_vector(&mut backend, &text)?;
        Ok(PreparedSearch {
            backend,
            vector,
            local,
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
        local,
        generation,
    } = prepared;
    let Options {
        unit,
        fusion: _,
        ranking,
        shared_vector_search,
        output_redaction: _,
    } = options;
    let local_filters = filters.clone();
    let local_database = local_database.map(Path::to_path_buf);
    let context_database = local_database.clone();
    let exclude_self = local_database.is_some();
    let local_query = query.to_owned();
    let local_vector = vector.clone();
    let local = async move {
        retrieve_local_candidates(
            local.as_ref(),
            &local_query,
            &local_vector,
            &local_filters,
            options,
        )
        .await
    };
    let request = protocol::Search {
        match_mode: filters.match_mode,
        vector_policy: shared_vector_search,
        intent: crate::views::exact::Intent::RelatedDiscussion,
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
        let selection = if ranking == crate::views::config::Ranking::Fusion {
            crate::search_ranking::Selection {
                hits: fused,
                count: 0,
            }
        } else {
            crate::search_ranking::select(&mut backend, &ranking_query, fused, unit)?
        };
        Ok((backend, selection))
    })
    .await?;
    if ranking == crate::views::config::Ranking::Fusion {
        return finish_fused(backend, selection.hits, retrieval, options).await;
    }
    let (context, context_error) = measured_context_documents(
        config,
        context_database,
        &query,
        &selection.hits[..selection.count],
    )
    .await?;
    documents.extend(context);
    crate::blocking(move || {
        let fused = rerank_combined(&mut backend, &query, selection, &documents, options)?;
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
            hits: crate::search_ranking::finish_fusion(hits, options.fusion),
            retrieval,
            context_error: None,
        })
    })
    .await
}

async fn measured_context_documents(
    config: &SharedConfig,
    database: Option<std::path::PathBuf>,
    query: &str,
    hits: &[SearchHit],
) -> Result<(HashMap<(String, String), String>, Option<String>), AppError> {
    let mut stage = crate::observability::retrieval::Guard::begin(
        crate::observability::retrieval::Stage::Context,
    );
    let result = context_documents(config, database, query, hits).await?;
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
            let storage = Storage::open_silver(&database)?;
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
async fn retrieve_local_candidates(
    local: Option<&crate::views::local::Local>,
    query: &str,
    vector: &[f64],
    filters: &crate::search_filter::Scope,
    options: Options,
) -> Result<(Vec<SearchHit>, Vec<String>), AppError> {
    let Some(local) = local else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut candidates = local
        .candidates(protocol::Search {
            match_mode: filters.match_mode,
            vector_policy: None,
            intent: crate::views::exact::Intent::RelatedDiscussion,
            query: query.into(),
            unit: options.unit,
            generation: local.scope.generation.clone(),
            vector: vector.to_vec(),
            provider: None,
            days: None,
            exclude_self: false,
            filter: Some(filters.filter.clone()),
        })
        .await?;
    for hit in candidates
        .lexical
        .iter_mut()
        .chain(&mut candidates.semantic)
    {
        hit.node_id = None;
    }
    let hits = semantic::fuse(
        candidates.lexical,
        candidates.semantic,
        options.unit,
        options.fusion,
    );
    let documents = hits.iter().map(|hit| hit.content.clone()).collect();
    Ok((hits, documents))
}

#[tracing::instrument(name = "rerank", skip_all)]
fn rerank_combined(
    backend: &mut semantic::Backend,
    query: &str,
    selection: crate::search_ranking::Selection,
    documents: &HashMap<(String, String), String>,
    options: Options,
) -> Result<Vec<SearchHit>, AppError> {
    let Options { unit, fusion, .. } = options;
    let selected = selection.hits[..selection.count]
        .iter()
        .map(|hit| {
            documents
                .get(&(hit.node_id.clone().unwrap_or_default(), hit.id.clone()))
                .cloned()
                .ok_or_else(|| AppError::shared("shared candidate has no search projection"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::search_ranking::score(backend, query, selection, &selected, unit, fusion)
}
