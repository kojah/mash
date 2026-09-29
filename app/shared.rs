use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use postgres::{Client, Config, Row};
use serde::Serialize;

use crate::AppError;
use crate::config::SharedConfig;
use crate::semantic::{self, Models};
use crate::storage::{Message, SearchHit, Storage};

#[derive(Debug, Serialize)]
pub(crate) struct Publication {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Publication {
    pub(crate) const fn pending() -> Self {
        Self {
            state: "pending",
            error: None,
        }
    }
}

fn pg_error(error: postgres::Error) -> AppError {
    // Server error details can include uploaded content. Never echo the URL or raw errors.
    let code = error
        .code()
        .map_or("connection", postgres::error::SqlState::code);
    let result = if code == "42P01" {
        AppError::shared("shared index is not initialized; run `mash index` on a configured node")
    } else {
        AppError::shared(format!("PostgreSQL operation failed ({code})"))
    };
    drop(error);
    result
}

fn connection(config: &SharedConfig) -> Result<(Client, String), AppError> {
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
    if options.get_ssl_mode() != postgres::config::SslMode::Disable {
        options.ssl_mode(postgres::config::SslMode::Require);
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
    let mut client = options
        .connect(tokio_postgres_rustls::MakeRustlsConnect::new(tls))
        .map_err(pg_error)?;
    let schema = group_schema(config);
    client
        .batch_execute(&format!("SET search_path = {schema};"))
        .map_err(pg_error)?;
    client.batch_execute("SET statement_timeout = '30s'; SET lock_timeout = '5s'; SET idle_in_transaction_session_timeout = '60s'")
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

pub(crate) fn publish(storage: &mut Storage, config: &SharedConfig) -> Publication {
    match publish_result(storage, config) {
        Ok(()) => Publication {
            state: "published",
            error: None,
        },
        Err(error) => Publication {
            state: "pending",
            error: Some(error.error.message),
        },
    }
}

fn publish_result(storage: &Storage, config: &SharedConfig) -> Result<(), AppError> {
    let (mut client, target) = connection(config)?;
    // Serialize schema initialization so first-time publishers cannot race DDL.
    let mut schema = client.transaction().map_err(pg_error)?;
    schema
        .query_one("SELECT pg_advisory_xact_lock(1730294612)", &[])
        .map_err(pg_error)?;
    schema
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {};",
            group_schema(config)
        ))
        .map_err(pg_error)?;
    schema
        .batch_execute(include_str!("shared/schema.sql"))
        .map_err(pg_error)?;
    validate_version(&mut schema)?;
    schema.commit().map_err(pg_error)?;
    let revision = storage.with_publication_snapshot(|snapshot| {
        let mut transaction = client.transaction().map_err(pg_error)?;
        transaction.execute(
            "INSERT INTO nodes VALUES ($1, $2, 0) ON CONFLICT DO NOTHING",
            &[&config.node_id, &snapshot.instance],
        ).map_err(pg_error)?;
        let node = transaction.query_one(
            "SELECT instance, revision FROM nodes WHERE node_id = $1 FOR UPDATE", &[&config.node_id],
        ).map_err(pg_error)?;
        if node.try_get::<_, String>(0).map_err(pg_error)? != snapshot.instance {
            return Err(AppError::shared("node_id belongs to another local database; choose a new node_id"));
        }
        let accepted: i64 = node.try_get(1).map_err(pg_error)?;
        if accepted > snapshot.revision {
            return Err(AppError::shared("shared index has a newer snapshot; local database may have been restored"));
        }
        if accepted == snapshot.revision { return transaction.commit().map_err(pg_error); }
        snapshot.visit("tombstones", |batch| {
            let json = serde_json::to_string(batch).map_err(|_| AppError::internal("serialize publication"))?;
            transaction.execute(
                "INSERT INTO tombstones SELECT $1, provider, conversation_id
                 FROM jsonb_to_recordset($2::text::jsonb) AS r(provider text, conversation_id text)
                 ON CONFLICT DO NOTHING", &[&config.node_id, &json],
            ).map_err(pg_error)?;
            Ok(())
        })?;
        transaction.execute("DELETE FROM conversations WHERE node_id = $1", &[&config.node_id]).map_err(pg_error)?;
        snapshot.visit("conversations", |batch| {
            let json = serde_json::to_string(batch).map_err(|_| AppError::internal("serialize publication"))?;
            transaction.execute(
                "INSERT INTO conversations SELECT $1, r.id, r.provider, title, created_at, updated_at
                 FROM jsonb_to_recordset($2::text::jsonb) AS r(id text, provider text, title text, created_at bigint, updated_at bigint)
                 WHERE NOT EXISTS (SELECT 1 FROM tombstones t WHERE t.node_id = $1 AND t.provider = r.provider AND t.conversation_id = r.id)",
                &[&config.node_id, &json],
            ).map_err(pg_error)?;
            Ok(())
        })?;
        snapshot.visit("messages", |batch| {
            let json = serde_json::to_string(batch).map_err(|_| AppError::internal("serialize publication"))?;
            transaction.execute(
                "INSERT INTO messages(node_id,id,conversation_id,ordinal,role,content,projection,created_at,generation,vector)
                 SELECT $1, r.id, r.conversation_id, ordinal, role, content, projection, created_at, generation, vector
                 FROM jsonb_to_recordset($2::text::jsonb) AS r(id text, conversation_id text, ordinal bigint, role text,
                      content text, projection text, created_at bigint, generation text, vector double precision[])
                 WHERE EXISTS (SELECT 1 FROM conversations c WHERE c.node_id = $1 AND c.id = r.conversation_id)",
                &[&config.node_id, &json],
            ).map_err(pg_error)?;
            Ok(())
        })?;
        transaction.execute("UPDATE nodes SET revision = $2 WHERE node_id = $1", &[&config.node_id, &snapshot.revision]).map_err(pg_error)?;
        transaction.commit().map_err(pg_error)
    })?;
    storage.acknowledge_publication(&target, revision)
}

fn validate_version(client: &mut impl postgres::GenericClient) -> Result<(), AppError> {
    let version: i32 = client
        .query_one("SELECT version FROM version WHERE singleton", &[])
        .map_err(pg_error)?
        .try_get(0)
        .map_err(pg_error)?;
    if version != 1 {
        return Err(AppError::shared("unsupported shared index schema version"));
    }
    Ok(())
}

pub(crate) fn view(
    config: &SharedConfig,
    node: &str,
    id: &str,
    context: u32,
) -> Result<Vec<Message>, AppError> {
    let (mut client, _) = connection(config)?;
    validate_version(&mut client)?;
    client.query(
        "SELECT m.id,m.ordinal,m.role,m.content,m.created_at FROM messages m
         JOIN messages anchor ON anchor.node_id = m.node_id AND anchor.conversation_id = m.conversation_id
         WHERE anchor.node_id = $1 AND anchor.id = $2
           AND m.ordinal BETWEEN anchor.ordinal - $3 AND anchor.ordinal + $3 ORDER BY m.ordinal",
        &[&node, &id, &i64::from(context)],
    ).map_err(pg_error)?.iter().map(|row| Ok(Message {
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

fn candidates(
    client: &mut impl postgres::GenericClient,
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
        .map_err(pg_error)?;
    let semantic = client.query(
        "SELECT m.node_id,m.id,m.conversation_id,c.provider,m.role,m.content,m.projection,
                COALESCE((SELECT sum(a*b)/NULLIF(sqrt(sum(a*a)*sum(b*b)),0)
                          FROM unnest(m.vector,$1::double precision[]) AS v(a,b)),0) AS score
         FROM messages m JOIN conversations c ON c.node_id=m.node_id AND c.id=m.conversation_id
         WHERE generation=$2 AND cardinality(vector)=cardinality($1::double precision[]) AND projection <> ''
           AND ($3::text IS NULL OR c.provider=$3) AND ($4::bigint IS NULL OR m.created_at >= $4)
         ORDER BY score DESC,m.node_id,m.id LIMIT 50", &[&vector, &generation, &provider, &cutoff],
    ).map_err(pg_error)?;
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

pub(crate) fn search(
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
    let models = Models::new(models_dir.to_path_buf());
    models.require_installed()?;
    let mut backend = models
        .load()?
        .ok_or_else(|| AppError::model("run `mash models install`"))?;
    let vector = semantic::shared_query_vector(&mut backend, query)?;
    let (mut client, _) = connection(config)?;
    validate_version(&mut client)?;
    let mut transaction = client
        .build_transaction()
        .isolation_level(postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .map_err(pg_error)?;
    let dimensions =
        i32::try_from(vector.len()).map_err(|_| AppError::model("invalid embedding dimensions"))?;
    let incompatible: bool = transaction
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE projection <> '' AND
         (generation IS DISTINCT FROM $1 OR vector IS NULL OR cardinality(vector) <> $2))",
            &[&semantic::SHARED_EMBEDDING_GENERATION, &dimensions],
        )
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
    } = candidates(&mut transaction, query, &vector, provider, days)?;
    transaction.commit().map_err(pg_error)?;
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
