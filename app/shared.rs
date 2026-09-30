use crate::AppError;
use crate::config::SharedConfig;
use crate::semantic::{self, Models};
use crate::storage::{Message, SearchHit, Storage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;
mod backend;
pub(crate) mod direct;
mod protocol;
mod redis_index;
pub(crate) mod server;
use protocol::{Event, Start};
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Publication {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Publication {
    pub(crate) fn is_pending(&self) -> bool {
        self.state == "pending"
    }
    pub(crate) const fn published() -> Self {
        Self {
            state: "published",
            error: None,
        }
    }
    pub(crate) const fn pending() -> Self {
        Self {
            state: "pending",
            error: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Freshness {
    group_id: String,
    nodes: Vec<NodeFreshness>,
}

#[derive(Debug, Deserialize, Serialize)]
struct NodeFreshness {
    node_id: String,
    revision: i64,
    last_synced_at: Option<String>,
    last_updated_at: Option<String>,
    conversations: i64,
    messages: i64,
}

pub(crate) async fn freshness(config: &SharedConfig) -> Result<Freshness, AppError> {
    get(config, "freshness").await
}
fn target(config: &SharedConfig) -> String {
    blake3::hash(
        format!(
            "{}\0{}\0{}",
            config.group_id,
            config.node_id,
            config.api_url.trim_end_matches('/')
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string()
}
// Keep the transport pool alive across MCP calls and watch refreshes. Identity
// headers belong to each request so configuration/token changes cannot retain
// another node's credentials in a cached client.
static HTTP_CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(600))
        .build()
});

fn request(
    config: &SharedConfig,
    method: reqwest::Method,
    route: &str,
) -> Result<reqwest::RequestBuilder, AppError> {
    let token = std::env::var(&config.token_env).map_err(|_| {
        AppError::shared(format!("set {} to this node's API token", config.token_env))
    })?;
    let mut headers = reqwest::header::HeaderMap::new();
    let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| AppError::shared("invalid API token"))?;
    authorization.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, authorization);
    for (key, value) in [
        ("x-mash-group", &config.group_id),
        ("x-mash-node", &config.node_id),
    ] {
        headers.insert(
            key,
            reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| AppError::shared("invalid API identity"))?,
        );
    }
    let client = HTTP_CLIENT
        .as_ref()
        .map_err(|_| AppError::shared("could not configure HTTP client"))?;
    Ok(client
        .request(
            method,
            format!("{}/v1/{route}", config.api_url.trim_end_matches('/')),
        )
        .headers(headers))
}
async fn response<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, AppError> {
    let response = request.send().await.map_err(|_| {
        AppError::shared("shared index HTTP request failed; check the API address and network")
    })?;
    if !response.status().is_success() {
        let status = response.status();
        let message = match status.as_u16() {
            401 | 403 => {
                "shared index authorization failed; check node token and group configuration"
            }
            409 => {
                "shared index revision or node identity conflict; retry indexing or choose a new node_id"
            }
            400 | 413 | 422 => "shared index rejected invalid or oversized data",
            _ => "shared index service is unavailable; retry later",
        };
        let mut error = AppError::shared(message);
        error.error.retryable = !matches!(status.as_u16(), 400 | 401 | 403 | 413 | 422);
        return Err(error);
    }
    response
        .json()
        .await
        .map_err(|_| AppError::shared("invalid shared index HTTP response"))
}
async fn get<T: serde::de::DeserializeOwned>(
    config: &SharedConfig,
    route: &str,
) -> Result<T, AppError> {
    response(request(config, reqwest::Method::GET, route)?).await
}
async fn post<T: serde::de::DeserializeOwned>(
    config: &SharedConfig,
    route: &str,
    body: &impl Serialize,
) -> Result<T, AppError> {
    response(request(config, reqwest::Method::POST, route)?.json(body)).await
}
fn write_event(file: &mut impl Write, event: &Event) -> Result<(), AppError> {
    serde_json::to_writer(&mut *file, event)
        .map_err(|_| AppError::internal("serialize index upload"))?;
    file.write_all(b"\n")
        .map_err(|_| AppError::internal("write index upload"))
}
async fn upload(
    config: &SharedConfig,
    file: &tempfile::NamedTempFile,
) -> Result<protocol::Indexed, AppError> {
    use futures_util::StreamExt;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    let total_bytes = file.as_file().metadata().map_err(AppError::io)?.len();
    let file = file
        .reopen()
        .map_err(|_| AppError::internal("open index upload"))?;
    let sent = Arc::new(AtomicU64::new(0));
    let stream_sent = sent.clone();
    let stream =
        tokio_util::io::ReaderStream::new(tokio::fs::File::from_std(file)).inspect(move |chunk| {
            if let Ok(chunk) = chunk {
                stream_sent.fetch_add(
                    u64::try_from(chunk.len()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            }
        });
    let request = response(
        request(config, reqwest::Method::POST, "index")?
            .header(reqwest::header::CONTENT_TYPE, "application/x-ndjson")
            .body(reqwest::Body::wrap_stream(stream)),
    );
    tokio::pin!(request);
    let started = std::time::Instant::now();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut estimate = crate::progress::Estimate::new();
    loop {
        tokio::select! {
            result = &mut request => return result,
            _ = interval.tick() => {
                let uploaded_bytes = sent.load(Ordering::Relaxed);
                let elapsed = crate::progress::milliseconds(started);
                let committing = uploaded_bytes >= total_bytes;
                let eta = if committing { None } else { estimate.observe(uploaded_bytes, total_bytes, elapsed) };
                crate::progress::emit(&serde_json::json!({"event":"index-progress",
                    "phase":if committing {"shared-commit"} else {"shared-upload"},
                    "uploaded_bytes":uploaded_bytes,"total_bytes":total_bytes,
                    "elapsed_milliseconds":elapsed,"eta_seconds":eta,"eta_scope":"phase"}));
            }
        }
    }
}
pub(crate) fn status(
    path: &Path,
    config: Option<&SharedConfig>,
) -> Result<Option<Publication>, AppError> {
    let Some(config) = config else {
        return Ok(None);
    };
    let Ok(_) = std::env::var(&config.token_env) else {
        return Ok(Some(Publication::pending()));
    };
    let pending = !path.is_file() || Storage::publication_is_pending(path, &target(config))?;
    Ok(Some(Publication {
        state: if pending { "pending" } else { "published" },
        error: None,
    }))
}

pub(crate) async fn publish(path: &Path, config: &SharedConfig) -> Publication {
    match publish_result(path, config).await {
        Ok(()) => Publication::published(),
        Err(error) => Publication {
            state: "pending",
            error: Some(error.error.message),
        },
    }
}

fn routed_target(configuration: &crate::config::ResolvedConfig) -> Result<String, AppError> {
    let config = configuration.shared.as_ref().ok_or_else(|| {
        AppError::configuration("shared destination requires shared configuration")
    })?;
    let filters = serde_json::to_vec(&(&configuration.local_filter, &configuration.shared_filter))
        .map_err(|_| AppError::internal("serialize routing settings"))?;
    Ok(format!(
        "{}:{}",
        target(config),
        blake3::hash(&filters).to_hex()
    ))
}

pub(crate) fn routed_scope(config: &SharedConfig) -> String {
    blake3::hash(format!("{}\0{}", config.group_id, config.node_id).as_bytes())
        .to_hex()
        .to_string()
}

pub(crate) fn routed_status(
    path: &Path,
    configuration: &crate::config::ResolvedConfig,
) -> Result<Publication, AppError> {
    let Ok(target) = routed_target(configuration) else {
        return Ok(Publication::pending());
    };
    if !path.is_file() || Storage::publication_is_pending(path, &target)? {
        Ok(Publication::pending())
    } else {
        Ok(Publication::published())
    }
}

pub(crate) async fn publish_routed(
    path: &Path,
    configuration: &crate::config::ResolvedConfig,
    models_dir: &Path,
    options: &crate::ingestion::IndexOptions,
    full: bool,
) -> Publication {
    let attempt = async {
        let config = configuration.shared.as_ref().ok_or_else(|| {
            AppError::configuration("shared destination requires shared configuration")
        })?;
        let scope = routed_scope(config);
        let db = path.to_path_buf();
        let owner =
            crate::blocking(move || Storage::open_existing(&db)?.direct_owner(&scope)).await?;
        let target = routed_target(configuration)?;
        direct::index(
            config,
            models_dir,
            configuration.embeddings.as_ref(),
            options,
            full,
            Some(&owner),
        )
        .await?;
        let db = path.to_path_buf();
        crate::blocking(move || {
            Storage::open_existing(&db)?.acknowledge_direct(&target, &owner, true)
        })
        .await?;
        Ok::<_, AppError>(())
    }
    .await;
    match attempt {
        Ok(()) => Publication::published(),
        Err(error) => Publication {
            state: "pending",
            error: Some(error.error.message),
        },
    }
}

pub(crate) async fn forget_routed(
    path: &Path,
    configuration: &crate::config::ResolvedConfig,
    id: &str,
) -> Result<(Publication, bool), AppError> {
    let config = configuration.shared.as_ref().ok_or_else(|| {
        AppError::configuration("shared destination requires shared configuration")
    })?;
    let scope = routed_scope(config);
    let db = path.to_path_buf();
    let id = id.to_owned();
    let owner = crate::blocking(move || {
        let storage = Storage::open_existing(&db)?;
        storage.queue_shared_forget(&scope, &id)?;
        storage.direct_owner(&scope)
    })
    .await?;
    let attempt = async {
        let target = routed_target(configuration)?;
        let forgotten = direct::flush_forgets(config, &owner).await?;
        let db = path.to_path_buf();
        crate::blocking(move || {
            Storage::open_existing(&db)?.acknowledge_direct(&target, &owner, false)
        })
        .await?;
        Ok::<_, AppError>(forgotten)
    }
    .await;
    Ok(match attempt {
        Ok(forgotten) => (Publication::published(), forgotten),
        Err(error) => (
            Publication {
                state: "pending",
                error: Some(error.error.message),
            },
            false,
        ),
    })
}

async fn publish_result(path: &Path, config: &SharedConfig) -> Result<(), AppError> {
    // Parse and spool before starting a remote transaction. Temporary files contain
    // only the outgoing data, have restrictive permissions, and are deleted on drop.
    let db = path.to_path_buf();
    let (file, revision) = crate::blocking(move || {
        crate::progress::during("shared-snapshot", || {
            let mut file = tempfile::NamedTempFile::new()
                .map_err(|_| AppError::internal("create index upload"))?;
            let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file.as_file_mut());
            let revision = Storage::open_existing(&db)?.with_publication_snapshot(|snapshot| {
                write_event(
                    &mut writer,
                    &Event::Start(Start {
                        instance: snapshot.instance.clone(),
                        revision: Some(snapshot.revision),
                        base_revision: None,
                        snapshot: true,
                        forgets: Vec::new(),
                    }),
                )?;
                for kind in ["tombstones", "conversations", "messages"] {
                    snapshot.visit(kind, |records| {
                        write_event(
                            &mut writer,
                            &Event::Batch {
                                kind: kind.to_owned(),
                                records: records.to_vec(),
                            },
                        )
                    })?;
                }
                write_event(&mut writer, &Event::Finish)
            })?;
            writer
                .flush()
                .map_err(|_| AppError::internal("flush index upload"))?;
            drop(writer);
            Ok((file, revision))
        })
    })
    .await?;
    upload(config, &file).await?;
    let db = path.to_path_buf();
    let target = target(config);
    crate::blocking(move || Storage::open_existing(&db)?.acknowledge_publication(&target, revision))
        .await
}
pub(crate) async fn view(
    config: &SharedConfig,
    node: &str,
    id: &str,
    context: u32,
) -> Result<Vec<Message>, AppError> {
    post(
        config,
        "view",
        &protocol::View {
            node_id: node.to_owned(),
            id: id.to_owned(),
            context,
        },
    )
    .await
}
pub(crate) async fn search(
    config: &SharedConfig,
    local_database: Option<&Path>,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    query: &str,
    limit: usize,
    filters: &crate::search_filter::Scope,
) -> Result<(Vec<SearchHit>, Option<String>), AppError> {
    filters.filter.validate()?;
    if query.trim().is_empty() {
        return Err(AppError::usage("search query must not be empty"));
    }
    let prepared = prepare_search(local_database, models_dir, embeddings, query).await?;
    search_prepared(config, local_database, query, limit, filters, prepared).await
}

struct PreparedSearch {
    backend: semantic::LoadedBackend,
    vector: Vec<f64>,
    local_generation: String,
    generation: String,
}

async fn prepare_search(
    local_database: Option<&Path>,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    query: &str,
) -> Result<PreparedSearch, AppError> {
    let root = models_dir.to_path_buf();
    let text = query.to_owned();
    let embeddings = embeddings.cloned();
    let readiness_database = local_database.map(Path::to_path_buf);
    let generation = embeddings.as_ref().map_or_else(
        || semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
        |c| semantic::model_space(&c.model, c.dimensions),
    );
    crate::blocking(move || {
        let models = Models::new(root).with_remote(embeddings);
        models.require_installed()?;
        if let Some(database) = readiness_database {
            Storage::open_search(&database, &models.generation())?;
        }
        let mut backend = models
            .load()?
            .ok_or_else(|| AppError::model("run `mash models install`"))?;
        let vector = semantic::shared_query_vector(&mut backend, &text)?;
        Ok(PreparedSearch {
            backend,
            vector,
            local_generation: models.generation(),
            generation,
        })
    })
    .await
}

async fn search_prepared(
    config: &SharedConfig,
    local_database: Option<&Path>,
    query: &str,
    limit: usize,
    filters: &crate::search_filter::Scope,
    prepared: PreparedSearch,
) -> Result<(Vec<SearchHit>, Option<String>), AppError> {
    let PreparedSearch {
        mut backend,
        vector,
        local_generation,
        generation,
    } = prepared;
    let local_filters = filters.clone();
    let local_database = local_database.map(Path::to_path_buf);
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
        )
    });
    let remote = async {
        // Keep transport errors as values until local retrieval finishes so
        // merged searches can still fall back to local results. A local error
        // cancels the HTTP request rather than waiting for its timeout.
        Ok::<_, AppError>(
            post::<protocol::Candidates>(
                config,
                "search",
                &protocol::Search {
                    query: query.to_owned(),
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
        lexical,
        semantic,
        documents,
    } = candidates;
    let mut documents: HashMap<_, _> = documents
        .into_iter()
        .map(|d| ((d.node_id, d.id), d.projection))
        .collect();
    for (hit, projection) in local_hits.iter().zip(local_documents) {
        documents.insert((String::new(), hit.id.clone()), projection);
    }
    let query = query.to_owned();
    crate::blocking(move || {
        let mut fused = semantic::fuse(lexical, semantic);
        fused.extend(local_hits);
        fused.sort_by(|a, b| {
            b.fusion_score
                .total_cmp(&a.fusion_score)
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.id.cmp(&b.id))
        });
        let fused = rerank_combined(&mut backend, &query, fused, &documents, limit)?;
        Ok((fused, shared_error))
    })
    .await
}

fn retrieve_local_candidates(
    database: Option<&Path>,
    generation: &str,
    query: &str,
    vector: &[f64],
    filters: &crate::search_filter::Scope,
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
        None,
        None,
        Some(filters),
    )
}

fn rerank_combined(
    backend: &mut semantic::Backend,
    query: &str,
    mut fused: Vec<SearchHit>,
    documents: &HashMap<(String, String), String>,
    limit: usize,
) -> Result<Vec<SearchHit>, AppError> {
    let count = fused.len().min(10);
    let documents = fused[..count]
        .iter()
        .map(|hit| {
            documents
                .get(&(hit.node_id.clone().unwrap_or_default(), hit.id.clone()))
                .map(String::as_str)
                .ok_or_else(|| AppError::shared("shared candidate has no search projection"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if count > 0 {
        for result in backend.rerank(query, &documents)? {
            if let Some(hit) = fused[..count].get_mut(result.index) {
                hit.rerank_score = Some(result.score);
            }
        }
        fused[..count].sort_by(|a, b| {
            b.rerank_score
                .unwrap_or(f32::NEG_INFINITY)
                .total_cmp(&a.rerank_score.unwrap_or(f32::NEG_INFINITY))
                .then_with(|| b.fusion_score.total_cmp(&a.fusion_score))
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.id.cmp(&b.id))
        });
    }
    fused.truncate(limit.min(1000));
    Ok(fused)
}

#[cfg(test)]
mod tests;
