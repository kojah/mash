//! Concurrent local/shared candidate retrieval, fusion and contextual ranking.
use super::client::{Endpoint, post};
use super::protocol;
use crate::semantic::{self, Models};
use crate::storage::{Message, SearchHit, Storage};
use crate::{AppError, config::SharedConfig};
use std::{collections::HashMap, path::Path, time::Duration};

pub(super) const CONTEXT_DEADLINE: Duration = Duration::from_secs(1);

#[tracing::instrument(name = "combined_search", skip_all)]
pub(crate) async fn search(
    config: &SharedConfig,
    local_database: Option<(&Path, &crate::gold::config::Build)>,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    query: &str,
    options: (usize, crate::search_results::SearchUnit),
    filters: &crate::search_filter::Scope,
) -> Result<(Vec<SearchHit>, Option<String>), AppError> {
    filters.filter.validate()?;
    crate::search_query::validate(query)?;
    let prepared = prepare_search(local_database, models_dir, embeddings, query).await?;
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
        models.require_installed()?;
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
            .load()?
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
    options: (usize, crate::search_results::SearchUnit),
    filters: &crate::search_filter::Scope,
    prepared: PreparedSearch,
) -> Result<(Vec<SearchHit>, Option<String>), AppError> {
    let PreparedSearch {
        mut backend,
        vector,
        local_generation,
        generation,
    } = prepared;
    let (limit, unit) = options;
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
        )
    });
    let remote = async {
        // Keep transport errors as values until local retrieval finishes so
        // merged searches can still fall back to local results. A local error
        // cancels the HTTP request rather than waiting for its timeout.
        Ok::<_, AppError>(
            post::<protocol::Candidates>(
                config,
                Endpoint::Search,
                &protocol::Search {
                    query: query.to_owned(),
                    unit,
                    vector,
                    generation,
                    provider: None,
                    days: None,
                    filter: Some(filters.filter.clone()),
                    exclude_self,
                },
            )
            .await,
        )
    };
    let ((local_hits, local_documents), candidates) = tokio::try_join!(local, remote)?;
    let (candidates, shared_error) = match candidates {
        Ok(candidates) => (candidates, None),
        Err(error) if exclude_self => (
            protocol::Candidates {
                lexical: Vec::new(),
                semantic: Vec::new(),
                documents: Vec::new(),
            },
            Some(error.error.message),
        ),
        Err(error) => return Err(error),
    };
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
        let mut fused = semantic::fuse(lexical, semantic, unit);
        fused.extend(local_hits);
        fused.sort_by(|a, b| {
            b.fusion_score
                .total_cmp(&a.fusion_score)
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.id.cmp(&b.id))
        });
        let fused = crate::search_results::diversify(fused, unit);
        let selection =
            crate::search_ranking::select(&mut backend, &ranking_query, fused, unit, limit)?;
        Ok((backend, selection))
    })
    .await?;
    let (context, context_error) = context_documents(
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
        Ok((fused, shared_error.or(context_error)))
    })
    .await
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
    use futures_util::{StreamExt, stream};
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
    let remote = async {
        let mut pending = stream::iter(anchors)
            .map(|(node, id)| {
                let config = config.clone();
                async move {
                    let result = post::<Vec<Message>>(
                        &config,
                        Endpoint::View,
                        &protocol::View {
                            node_id: node.clone(),
                            id: id.clone(),
                            context: 2,
                            content: crate::storage::MessageContent::SearchProjection,
                        },
                    )
                    .await;
                    ((node, id), result)
                }
            })
            .buffer_unordered(8);
        let deadline = tokio::time::sleep(CONTEXT_DEADLINE);
        tokio::pin!(deadline);
        let mut completed = Vec::new();
        loop {
            tokio::select! {
                result = pending.next() => match result {
                    Some(result) => completed.push(result),
                    None => return (completed, false),
                },
                () = &mut deadline => return (completed, true),
            }
        }
    };
    let (contexts, (remote, timed_out)) = tokio::join!(local, remote);
    let mut contexts = contexts?;
    let mut error = timed_out.then(|| {
        "shared context deadline exceeded; ranking uses available message evidence".to_owned()
    });
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
