use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tokio_postgres::{Client, Config, Row};

use crate::AppError;
use crate::config::SharedConfig;
use crate::semantic::{self, Models};
use crate::storage::{Message, SearchHit, Storage};

pub(crate) mod direct;

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub(crate) struct Freshness {
    group_id: String,
    nodes: Vec<NodeFreshness>,
}

#[derive(Debug, Serialize)]
struct NodeFreshness {
    node_id: String,
    revision: i64,
    last_synced_at: Option<String>,
    last_updated_at: Option<String>,
    conversations: i64,
    messages: i64,
}

pub(crate) async fn freshness(config: &SharedConfig) -> Result<Freshness, AppError> {
    let (mut client, _) = connection(config).await?;
    validate_version(&mut client).await?;
    let rows = client
        .query(
            "SELECT n.node_id,n.revision,
         to_char(s.last_synced_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
         to_char(s.last_updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
         (SELECT count(*) FROM conversations c WHERE c.node_id=n.node_id),
         (SELECT count(*) FROM messages m WHERE m.node_id=n.node_id)
         FROM nodes n LEFT JOIN node_sync s USING(node_id) ORDER BY n.node_id",
            &[],
        )
        .await
        .map_err(pg_error)?;
    let nodes = rows
        .into_iter()
        .map(|row| {
            Ok(NodeFreshness {
                node_id: row.try_get(0).map_err(pg_error)?,
                revision: row.try_get(1).map_err(pg_error)?,
                last_synced_at: row.try_get(2).map_err(pg_error)?,
                last_updated_at: row.try_get(3).map_err(pg_error)?,
                conversations: row.try_get(4).map_err(pg_error)?,
                messages: row.try_get(5).map_err(pg_error)?,
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(Freshness {
        group_id: config.group_id.clone(),
        nodes,
    })
}

async fn record_sync(
    client: &impl tokio_postgres::GenericClient,
    node: &str,
    changed: bool,
    checked: bool,
) -> Result<(), AppError> {
    client
        .execute(
            "INSERT INTO node_sync(node_id,last_synced_at,last_updated_at)
         VALUES ($1,CASE WHEN $3 THEN clock_timestamp() END,CASE WHEN $2 THEN clock_timestamp() END)
         ON CONFLICT(node_id) DO UPDATE SET
         last_synced_at=CASE WHEN $3 THEN clock_timestamp() ELSE node_sync.last_synced_at END,
         last_updated_at=CASE WHEN $2 THEN clock_timestamp() ELSE node_sync.last_updated_at END",
            &[&node, &changed, &checked],
        )
        .await
        .map_err(pg_error)?;
    Ok(())
}

fn pg_error(error: tokio_postgres::Error) -> AppError {
    // Server error details can include uploaded content. Never echo the URL or raw errors.
    let code = error
        .code()
        .map_or("connection", tokio_postgres::error::SqlState::code);
    let result = if code == "42P01" {
        AppError::shared("shared index is not initialized; run `mash index` on a configured node")
    } else {
        AppError::shared(format!("PostgreSQL operation failed ({code})"))
    };
    drop(error);
    result
}

async fn connection(config: &SharedConfig) -> Result<(Client, String), AppError> {
    let url = std::env::var(&config.database_url_env).map_err(|_| {
        AppError::shared(format!(
            "set {} to the shared PostgreSQL connection string",
            config.database_url_env
        ))
    })?;
    let mut options: Config = url
        .parse()
        .map_err(|_| AppError::shared("invalid shared PostgreSQL connection string"))?;
    // Encrypted by default; plaintext requires an explicit sslmode=disable.
    if options.get_ssl_mode() != tokio_postgres::config::SslMode::Disable {
        options.ssl_mode(tokio_postgres::config::SslMode::Require);
    }
    options.connect_timeout(Duration::from_secs(5));
    options.tcp_user_timeout(Duration::from_secs(30));
    options.application_name("mash");
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| AppError::shared("failed to configure shared TLS"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let (client, connection) = options
        .connect(tokio_postgres_rustls::MakeRustlsConnect::new(tls))
        .await
        .map_err(pg_error)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let schema = group_schema(config);
    client
        .batch_execute(&format!("SET search_path = {schema};"))
        .await
        .map_err(pg_error)?;
    client.batch_execute("SET statement_timeout = '30s'; SET lock_timeout = '5s'; SET idle_in_transaction_session_timeout = '60s'").await
        .map_err(pg_error)?;
    Ok((client, target(config, &url)))
}

fn group_schema(config: &SharedConfig) -> String {
    format!(
        "mash_g_{}",
        &blake3::hash(config.group_id.as_bytes()).to_hex()[..32]
    )
}

fn target(config: &SharedConfig, url: &str) -> String {
    blake3::hash(format!("{}\0{}\0{url}", config.group_id, config.node_id).as_bytes())
        .to_hex()
        .to_string()
}

pub(crate) fn status(
    path: &Path,
    config: Option<&SharedConfig>,
) -> Result<Option<Publication>, AppError> {
    let Some(config) = config else {
        return Ok(None);
    };
    let Ok(url) = std::env::var(&config.database_url_env) else {
        return Ok(Some(Publication::pending()));
    };
    let pending = !path.is_file() || Storage::publication_is_pending(path, &target(config, &url))?;
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
    let url = std::env::var(&config.database_url_env).map_err(|_| {
        AppError::shared(format!(
            "set {} to the shared PostgreSQL connection string",
            config.database_url_env
        ))
    })?;
    let filters = serde_json::to_vec(&(&configuration.local_filter, &configuration.shared_filter))
        .map_err(|_| AppError::internal("serialize routing settings"))?;
    Ok(format!(
        "{}:{}",
        target(config, &url),
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
        direct::index(config, models_dir, options, full, Some(&owner)).await?;
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

async fn initialize(client: &mut Client, config: &SharedConfig) -> Result<(), AppError> {
    // Serialize schema initialization so first-time publishers cannot race DDL.
    let mut schema = client.transaction().await.map_err(pg_error)?;
    schema
        .query_one("SELECT pg_advisory_xact_lock(1730294612)", &[])
        .await
        .map_err(pg_error)?;
    schema
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {};",
            group_schema(config)
        ))
        .await
        .map_err(pg_error)?;
    schema
        .batch_execute(include_str!("shared/schema.sql"))
        .await
        .map_err(pg_error)?;
    validate_version(&mut schema).await?;
    schema.commit().await.map_err(pg_error)
}

enum SnapshotEvent {
    Start { instance: String, revision: i64 },
    Batch { kind: &'static str, json: String },
}

async fn publish_result(path: &Path, config: &SharedConfig) -> Result<(), AppError> {
    let (mut client, target) = connection(config).await?;
    initialize(&mut client, config).await?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let db = path.to_path_buf();
    let producer = tokio::task::spawn_blocking(move || {
        Storage::open_existing(&db)?.with_publication_snapshot(|snapshot| {
            sender
                .blocking_send(SnapshotEvent::Start {
                    instance: snapshot.instance.clone(),
                    revision: snapshot.revision,
                })
                .map_err(|_| AppError::internal("snapshot receiver stopped"))?;
            for kind in ["tombstones", "conversations", "messages"] {
                snapshot.visit(kind, |batch| {
                    let json = serde_json::to_string(batch)
                        .map_err(|_| AppError::internal("serialize publication"))?;
                    sender
                        .blocking_send(SnapshotEvent::Batch { kind, json })
                        .map_err(|_| AppError::internal("snapshot receiver stopped"))
                })?;
            }
            Ok(())
        })
    });
    let Some(SnapshotEvent::Start { instance, revision }) = receiver.recv().await else {
        return producer
            .await
            .map_err(|_| AppError::internal("snapshot worker failed"))?
            .map(|_| ());
    };
    let transaction = client.transaction().await.map_err(pg_error)?;
    transaction
        .execute(
            "INSERT INTO nodes VALUES ($1,$2,0) ON CONFLICT DO NOTHING",
            &[&config.node_id, &instance],
        )
        .await
        .map_err(pg_error)?;
    let node = transaction
        .query_one(
            "SELECT instance,revision FROM nodes WHERE node_id=$1 FOR UPDATE",
            &[&config.node_id],
        )
        .await
        .map_err(pg_error)?;
    if node.try_get::<_, String>(0).map_err(pg_error)? != instance {
        return Err(AppError::shared(
            "node_id belongs to another local database; choose a new node_id",
        ));
    }
    let accepted: i64 = node.try_get(1).map_err(pg_error)?;
    if accepted > revision {
        return Err(AppError::shared(
            "shared index has a newer snapshot; local database may have been restored",
        ));
    }
    if accepted == revision {
        // The shared revision already exists; stop the bounded snapshot producer.
        drop(receiver);
        let _ = producer.await;
    } else {
        transaction
            .execute(
                "DELETE FROM conversations WHERE node_id=$1",
                &[&config.node_id],
            )
            .await
            .map_err(pg_error)?;
        while let Some(event) = receiver.recv().await {
            let SnapshotEvent::Batch { kind, json } = event else {
                return Err(AppError::internal("unexpected snapshot event"));
            };
            upload_batch(&transaction, &config.node_id, kind, &json).await?;
        }
        producer
            .await
            .map_err(|_| AppError::internal("snapshot worker failed"))??;
        transaction
            .execute(
                "UPDATE nodes SET revision=$2 WHERE node_id=$1",
                &[&config.node_id, &revision],
            )
            .await
            .map_err(pg_error)?;
    }
    record_sync(&transaction, &config.node_id, accepted != revision, true).await?;
    transaction.commit().await.map_err(pg_error)?;
    let db = path.to_path_buf();
    crate::blocking(move || Storage::open_existing(&db)?.acknowledge_publication(&target, revision))
        .await
}

async fn upload_batch(
    transaction: &tokio_postgres::Transaction<'_>,
    node: &str,
    kind: &str,
    json: &str,
) -> Result<(), AppError> {
    let sql = match kind {
        "tombstones" => "INSERT INTO tombstones SELECT $1, provider, conversation_id
            FROM jsonb_to_recordset($2::text::jsonb) AS r(provider text, conversation_id text) ON CONFLICT DO NOTHING",
        "conversations" => "INSERT INTO conversations SELECT $1,r.id,r.provider,title,created_at,updated_at
            FROM jsonb_to_recordset($2::text::jsonb) AS r(id text,provider text,title text,created_at bigint,updated_at bigint)
            WHERE NOT EXISTS(SELECT 1 FROM tombstones t WHERE t.node_id=$1 AND t.provider=r.provider AND t.conversation_id=r.id)",
        "messages" => "INSERT INTO messages(node_id,id,conversation_id,ordinal,role,content,projection,created_at,generation,vector)
            SELECT $1,r.id,r.conversation_id,ordinal,role,content,projection,created_at,generation,vector
            FROM jsonb_to_recordset($2::text::jsonb) AS r(id text,conversation_id text,ordinal bigint,role text,content text,projection text,created_at bigint,generation text,vector double precision[])
            WHERE EXISTS(SELECT 1 FROM conversations c WHERE c.node_id=$1 AND c.id=r.conversation_id)",
        _ => return Err(AppError::internal("unknown snapshot kind")),
    };
    transaction
        .execute(sql, &[&node, &json])
        .await
        .map_err(pg_error)?;
    Ok(())
}

async fn validate_version(
    client: &mut (impl tokio_postgres::GenericClient + Sync),
) -> Result<(), AppError> {
    let version: i32 = client
        .query_one("SELECT version FROM version WHERE singleton", &[])
        .await
        .map_err(pg_error)?
        .try_get(0)
        .map_err(pg_error)?;
    if version != 1 {
        return Err(AppError::shared("unsupported shared index schema version"));
    }
    Ok(())
}

pub(crate) async fn view(
    config: &SharedConfig,
    node: &str,
    id: &str,
    context: u32,
) -> Result<Vec<Message>, AppError> {
    let (mut client, _) = connection(config).await?;
    validate_version(&mut client).await?;
    client.query(
        "SELECT m.id,m.ordinal,m.role,m.content,m.created_at FROM messages m
         JOIN messages anchor ON anchor.node_id = m.node_id AND anchor.conversation_id = m.conversation_id
         WHERE anchor.node_id = $1 AND anchor.id = $2
           AND m.ordinal BETWEEN anchor.ordinal - $3 AND anchor.ordinal + $3 ORDER BY m.ordinal",
        &[&node, &id, &i64::from(context)],
    ).await.map_err(pg_error)?.iter().map(|row| Ok(Message {
        id: row.try_get(0).map_err(pg_error)?, ordinal: row.try_get(1).map_err(pg_error)?,
        role: row.try_get(2).map_err(pg_error)?, content: row.try_get(3).map_err(pg_error)?,
        created_at: row.try_get(4).map_err(pg_error)?,
    })).collect()
}

fn hit(row: &Row, lexical: bool) -> Result<SearchHit, AppError> {
    let score: f64 = row.try_get(7).map_err(pg_error)?;
    #[allow(clippy::cast_possible_truncation)]
    let semantic_score = (!lexical).then_some(score as f32);
    Ok(SearchHit {
        node_id: Some(row.try_get(0).map_err(pg_error)?),
        id: row.try_get(1).map_err(pg_error)?,
        conversation_id: row.try_get(2).map_err(pg_error)?,
        provider: row.try_get(3).map_err(pg_error)?,
        role: row.try_get(4).map_err(pg_error)?,
        content: row.try_get(5).map_err(pg_error)?,
        lexical_score: lexical.then_some(-score),
        semantic_score,
        fusion_score: 0.0,
        rerank_score: None,
    })
}

struct Candidates {
    lexical: Vec<SearchHit>,
    semantic: Vec<SearchHit>,
    documents: HashMap<(String, String), String>,
}

async fn candidates(
    client: &mut (impl tokio_postgres::GenericClient + Sync),
    query: &str,
    vector: &[f64],
    provider: Option<&str>,
    days: Option<u32>,
) -> Result<Candidates, AppError> {
    let cutoff = days.map(|days| {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::from(days) * 86400)
    });
    let generation = semantic::SHARED_EMBEDDING_GENERATION;
    let lexical = client
        .query(
            "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.content,m.projection,
                ts_rank_cd(document, websearch_to_tsquery('simple', $1))::double precision AS score
         FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
         WHERE document @@ websearch_to_tsquery('simple', $1) AND projection <> '' AND generation=$2
           AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at >= $4)
         ORDER BY score DESC,m.node_id,m.id LIMIT 50",
            &[&query, &generation, &provider, &cutoff],
        )
        .await
        .map_err(pg_error)?;
    let semantic = client.query(
        "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.content,m.projection,
                COALESCE((SELECT sum(a*b)/NULLIF(sqrt(sum(a*a)*sum(b*b)),0)
                          FROM unnest(m.vector,$1::double precision[]) AS v(a,b)),0) AS score
         FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
         WHERE generation=$2 AND cardinality(vector)=cardinality($1::double precision[]) AND projection <> ''
           AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at >= $4)
         ORDER BY score DESC,m.node_id,m.id LIMIT 50", &[&vector, &generation, &provider, &cutoff],
    ).await.map_err(pg_error)?;
    let mut documents = HashMap::new();
    for row in lexical.iter().chain(&semantic) {
        documents.insert(
            (
                row.try_get(0).map_err(pg_error)?,
                row.try_get(1).map_err(pg_error)?,
            ),
            row.try_get(6).map_err(pg_error)?,
        );
    }
    Ok(Candidates {
        lexical: lexical
            .iter()
            .map(|r| hit(r, true))
            .collect::<Result<_, _>>()?,
        semantic: semantic
            .iter()
            .map(|r| hit(r, false))
            .collect::<Result<_, _>>()?,
        documents,
    })
}

pub(crate) async fn search(
    config: &SharedConfig,
    models_dir: &Path,
    query: &str,
    limit: usize,
    provider: Option<&str>,
    days: Option<u32>,
) -> Result<Vec<SearchHit>, AppError> {
    if query.trim().is_empty() {
        return Err(AppError::usage("search query must not be empty"));
    }
    let root = models_dir.to_path_buf();
    let text = query.to_owned();
    let (mut backend, vector) = crate::blocking(move || {
        let models = Models::new(root);
        models.require_installed()?;
        let mut backend = models
            .load()?
            .ok_or_else(|| AppError::model("run `mash models install`"))?;
        let vector = semantic::shared_query_vector(&mut backend, &text)?;
        Ok((backend, vector))
    })
    .await?;
    let (mut client, _) = connection(config).await?;
    validate_version(&mut client).await?;
    let mut transaction = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await
        .map_err(pg_error)?;
    let dimensions =
        i32::try_from(vector.len()).map_err(|_| AppError::model("invalid embedding dimensions"))?;
    let incompatible: bool = transaction
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE projection <> '' AND
         (generation IS DISTINCT FROM $1 OR vector IS NULL OR cardinality(vector) <> $2))",
            &[&semantic::SHARED_EMBEDDING_GENERATION, &dimensions],
        )
        .await
        .map_err(pg_error)?
        .try_get(0)
        .map_err(pg_error)?;
    if incompatible {
        return Err(AppError::shared(
            "shared embeddings are incomplete or incompatible; run `mash index` on publishing nodes",
        ));
    }
    let Candidates {
        lexical,
        semantic,
        documents,
    } = candidates(&mut transaction, query, &vector, provider, days).await?;
    transaction.commit().await.map_err(pg_error)?;
    let query = query.to_owned();
    crate::blocking(move || {
        let mut fused = semantic::fuse(lexical, semantic);
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
            for result in backend.rerank(query.as_str(), &documents)? {
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
    })
    .await
}

#[cfg(test)]
mod tests;
