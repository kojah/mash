use std::path::Path;
use std::time::Instant;

use tokio_postgres::{GenericClient, Transaction};

use super::{Publication, connection, initialize, pg_error, validate_version};
use crate::AppError;
use crate::config::SharedConfig;
use crate::ingestion::{self, Conversation, DirectScan, IndexOptions, IndexSummary};
use crate::semantic::{self, EmbeddingPool, EmbeddingSummary, Models};
use crate::storage::Counts;
use crate::storage::publication::DirectOwner;

const DIRECT_INSTANCE: &str = "shared-only:v1";

pub(crate) struct IndexResult {
    pub(crate) counts: Counts,
    pub(crate) summary: IndexSummary,
    pub(crate) embeddings: EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) storage_setup_milliseconds: u64,
    pub(crate) ingestion_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

fn elapsed(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

// A node lock serializes indexing and forget across processes. Its identity lives
// entirely in PostgreSQL; no local database or retry journal is opened.
async fn lock_node(
    transaction: &mut Transaction<'_>,
    config: &SharedConfig,
    owner: Option<&DirectOwner>,
) -> Result<(), AppError> {
    let expected = owner.map_or(DIRECT_INSTANCE, |owner| owner.instance.as_str());
    transaction
        .execute(
            "INSERT INTO nodes VALUES ($1,$2,0) ON CONFLICT DO NOTHING",
            &[&config.node_id, &expected],
        )
        .await
        .map_err(pg_error)?;
    let row = transaction
        .query_one(
            "SELECT instance,revision FROM nodes WHERE node_id=$1 FOR UPDATE",
            &[&config.node_id],
        )
        .await
        .map_err(pg_error)?;
    let instance: String = row.try_get(0).map_err(pg_error)?;
    if instance != expected {
        return Err(AppError::shared(
            "node_id belongs to another database or indexing mode; choose a new node_id",
        ));
    }
    if let Some(owner) = owner {
        let accepted: i64 = row.try_get(1).map_err(pg_error)?;
        if accepted >= owner.revision {
            return Err(AppError::shared(
                "shared index has a newer revision; retry index or check whether SQLite was restored",
            ));
        }
    }
    Ok(())
}

pub(crate) async fn index(
    config: &SharedConfig,
    models_dir: &Path,
    options: &IndexOptions,
    full: bool,
    owner: Option<&DirectOwner>,
) -> Result<IndexResult, AppError> {
    let root = models_dir.to_path_buf();
    let input = options.clone();
    let scans = crate::blocking(move || {
        Models::new(root).require_installed()?;
        ingestion::direct_scans(&input)
    })
    .await?;
    let storage_started = Instant::now();
    let (mut client, _) = connection(config).await?;
    initialize(&mut client, config).await?;
    let mut transaction = client.transaction().await.map_err(pg_error)?;
    // Model loading/inference can legitimately take longer than the idle timeout.
    transaction
        .batch_execute("SET LOCAL idle_in_transaction_session_timeout = 0")
        .await
        .map_err(pg_error)?;
    lock_node(&mut transaction, config, owner).await?;
    if let Some(owner) = owner {
        for id in &owner.forgets {
            forget_in_transaction(&mut transaction, config, id).await?;
        }
    }
    let mut result = IndexResult {
        counts: Counts {
            conversations: 0,
            messages: 0,
            searchable_messages: 0,
            embeddings: 0,
        },
        summary: IndexSummary::default(),
        embeddings: EmbeddingSummary::default(),
        model_load_milliseconds: 0,
        storage_setup_milliseconds: elapsed(storage_started),
        ingestion_milliseconds: 0,
        embedding_milliseconds: 0,
    };
    let mut pool = None;
    let ingestion_started = Instant::now();
    for scan in &scans {
        ingest_scan(
            &mut transaction,
            config,
            scan,
            full,
            models_dir,
            &mut pool,
            &mut result,
        )
        .await?;
    }
    result.ingestion_milliseconds = elapsed(ingestion_started)
        .saturating_sub(result.model_load_milliseconds)
        .saturating_sub(result.embedding_milliseconds);
    result.counts = counts(&mut transaction, config).await?;
    if let Some(owner) = owner {
        transaction
            .execute(
                "UPDATE nodes SET revision=$2 WHERE node_id=$1",
                &[&config.node_id, &owner.revision],
            )
            .await
            .map_err(pg_error)?;
    } else {
        transaction
            .execute(
                "UPDATE nodes SET revision=revision+1 WHERE node_id=$1",
                &[&config.node_id],
            )
            .await
            .map_err(pg_error)?;
    }
    transaction.commit().await.map_err(pg_error)?;
    result.summary.committed_batches = 1;
    Ok(result)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one concrete ingestion transaction, without provider abstractions"
)]
async fn ingest_scan(
    transaction: &mut Transaction<'_>,
    config: &SharedConfig,
    scan: &DirectScan,
    full: bool,
    models_dir: &Path,
    pool: &mut Option<EmbeddingPool>,
    result: &mut IndexResult,
) -> Result<(), AppError> {
    let mut observed = Vec::new();
    for path in &scan.files {
        let hash = blake3::hash(path.to_string_lossy().as_bytes())
            .to_hex()
            .to_string();
        observed.push(hash.clone());
        let source = path.clone();
        let (size, modified) = crate::blocking(move || ingestion::source_stamp(&source)).await?;
        if scan.cutoff.is_some_and(|cutoff| modified < cutoff) {
            continue;
        }
        result.summary.scanned_files += 1;
        result.summary.discovered_bytes += u64::try_from(size).unwrap_or_default();
        result.summary.processed_files += 1;
        result.summary.processed_bytes += u64::try_from(size).unwrap_or_default();
        let checkpoint = transaction.query_opt(
            "SELECT size_bytes, modified_ns, NOT EXISTS (
                SELECT 1 FROM messages m WHERE m.node_id=s.node_id AND m.conversation_id=s.conversation_id
                AND m.projection <> '' AND (generation IS DISTINCT FROM $4 OR COALESCE(array_length(vector,1),0)<>384)
             ) FROM direct_sources s WHERE node_id = $1 AND provider = $2 AND path_hash = $3",
            &[&config.node_id, &scan.provider, &hash, &semantic::SHARED_EMBEDDING_GENERATION],
        ).await.map_err(pg_error)?;
        let unchanged = checkpoint
            .as_ref()
            .map(|row| {
                Ok::<_, AppError>(
                    row.try_get::<_, i64>(0).map_err(pg_error)? == size
                        && row.try_get::<_, i64>(1).map_err(pg_error)? == modified
                        && row.try_get::<_, bool>(2).map_err(pg_error)?,
                )
            })
            .transpose()?
            .unwrap_or(false);
        if !full && unchanged {
            result.summary.unchanged_sources += 1;
            result.summary.checkpoint_skipped_sources += 1;
            continue;
        }
        let source = path.clone();
        let provider = scan.provider;
        let parsed = crate::blocking(move || {
            let parsed = DirectScan::parse_path(provider, &source)?;
            if ingestion::source_stamp(&source)? != (size, modified) {
                return Err(AppError::shared(
                    "history changed while indexing; retry index",
                ));
            }
            Ok(parsed)
        })
        .await?;
        if parsed.malformed_records != 0 {
            return Err(AppError::shared(
                "shared-only indexing encountered malformed history; shared changes were rolled back",
            ));
        }
        let conversation_id = parsed.conversation.as_ref().map(|c| c.id.as_str());
        let previous = transaction.query_opt(
            "SELECT conversation_id FROM direct_sources WHERE node_id = $1 AND provider = $2 AND path_hash = $3",
            &[&config.node_id, &scan.provider, &hash],
        ).await.map_err(pg_error)?.map(|r| r.try_get::<_, Option<String>>(0).map_err(pg_error)).transpose()?.flatten();
        if let Some(previous) = previous
            .as_deref()
            .filter(|id| Some(*id) != conversation_id)
        {
            remove_conversation(transaction, config, previous, result).await?;
        }
        if let Some(conversation) = &parsed.conversation {
            replace_conversation(transaction, config, conversation, models_dir, pool, result)
                .await?;
        }
        transaction.execute(
            "INSERT INTO direct_sources VALUES ($1,$2,$3,$4,$5,$6)
             ON CONFLICT (node_id,provider,path_hash) DO UPDATE SET size_bytes=$4,modified_ns=$5,conversation_id=$6",
            &[&config.node_id, &scan.provider, &hash, &size, &modified, &conversation_id],
        ).await.map_err(pg_error)?;
    }
    let verification = scan.clone();
    crate::blocking(move || verification.verify()).await?;
    if scan.complete {
        let missing = transaction.query(
            "DELETE FROM direct_sources WHERE node_id=$1 AND provider=$2 AND NOT(path_hash=ANY($3))
             AND ($4::bigint IS NULL OR modified_ns >= $4) RETURNING conversation_id",
            &[&config.node_id, &scan.provider, &observed, &scan.cutoff],
        ).await.map_err(pg_error)?;
        for row in missing {
            if let Some(id) = row.try_get::<_, Option<String>>(0).map_err(pg_error)? {
                remove_conversation(transaction, config, &id, result).await?;
            }
        }
    }
    Ok(())
}

async fn remove_conversation(
    transaction: &mut Transaction<'_>,
    config: &SharedConfig,
    id: &str,
    result: &mut IndexResult,
) -> Result<(), AppError> {
    result.summary.removed_messages += transaction
        .query_one(
            "SELECT count(*) FROM messages WHERE node_id=$1 AND conversation_id=$2",
            &[&config.node_id, &id],
        )
        .await
        .map_err(pg_error)?
        .try_get::<_, i64>(0)
        .map_err(pg_error)?
        .cast_unsigned();
    result.summary.purged_conversations += transaction
        .execute(
            "DELETE FROM conversations WHERE node_id=$1 AND id=$2",
            &[&config.node_id, &id],
        )
        .await
        .map_err(pg_error)?;
    Ok(())
}

async fn embed_texts(
    models_dir: &Path,
    pool: &mut Option<EmbeddingPool>,
    texts: &[&str],
    result: &mut IndexResult,
) -> Result<Vec<Vec<f64>>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let existing = pool.take();
    let root = models_dir.to_path_buf();
    let input = texts
        .iter()
        .map(|text| (*text).to_owned())
        .collect::<Vec<_>>();
    let (loaded, vectors, load_ms, embed_ms) = crate::blocking(move || {
        let loading = existing.is_none();
        let load_started = Instant::now();
        let mut loaded = match existing {
            Some(pool) => pool,
            None => Models::new(root).load_indexer()?.ok_or_else(|| {
                AppError::model("semantic models are not installed; run `mash models install`")
            })?,
        };
        let load_ms = if loading { elapsed(load_started) } else { 0 };
        let references = input.iter().map(String::as_str).collect::<Vec<_>>();
        let embed_started = Instant::now();
        let vectors = loaded.shared_vectors(&references)?;
        Ok((loaded, vectors, load_ms, elapsed(embed_started)))
    })
    .await?;
    *pool = Some(loaded);
    result.model_load_milliseconds += load_ms;
    result.embedding_milliseconds += embed_ms;
    Ok(vectors)
}

async fn replace_conversation(
    transaction: &mut Transaction<'_>,
    config: &SharedConfig,
    conversation: &Conversation,
    models_dir: &Path,
    pool: &mut Option<EmbeddingPool>,
    result: &mut IndexResult,
) -> Result<(), AppError> {
    let forgotten = transaction
        .query_opt(
            "SELECT 1 FROM tombstones WHERE node_id=$1 AND provider=$2 AND conversation_id=$3",
            &[&config.node_id, &conversation.provider, &conversation.id],
        )
        .await
        .map_err(pg_error)?
        .is_some();
    if forgotten {
        result.summary.tombstoned_sources += 1;
        return Ok(());
    }
    transaction
        .execute(
            "DELETE FROM conversations WHERE node_id=$1 AND id=$2",
            &[&config.node_id, &conversation.id],
        )
        .await
        .map_err(pg_error)?;
    transaction
        .execute(
            "INSERT INTO conversations VALUES ($1,$2,$3,$4,$5,$6)",
            &[
                &config.node_id,
                &conversation.id,
                &conversation.provider,
                &conversation.title,
                &conversation.created_at,
                &conversation.updated_at,
            ],
        )
        .await
        .map_err(pg_error)?;
    for batch in conversation.messages.chunks(32) {
        let texts = batch
            .iter()
            .map(|m| m.search_projection.as_deref().unwrap_or(&m.content))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();
        let mut vectors = embed_texts(models_dir, pool, &texts, result)
            .await?
            .into_iter();
        result.embeddings.model_inferences += texts.len() as u64;
        for message in batch {
            let projection = message
                .search_projection
                .as_deref()
                .unwrap_or(&message.content);
            let vector = if projection.is_empty() {
                None
            } else {
                Some(
                    vectors
                        .next()
                        .ok_or_else(|| AppError::model("missing shared message embedding"))?,
                )
            };
            let generation = vector
                .as_ref()
                .map(|_| semantic::SHARED_EMBEDDING_GENERATION);
            transaction.execute(
                "INSERT INTO messages (node_id,id,conversation_id,ordinal,role,content,projection,created_at,generation,vector)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
                &[&config.node_id, &message.id, &conversation.id, &message.ordinal, &message.role,
                  &message.content, &projection, &message.created_at, &generation, &vector],
            ).await.map_err(pg_error)?;
            result.summary.changed_messages += 1;
            result.embeddings.stored_vectors += u64::from(vector.is_some());
        }
    }
    Ok(())
}

pub(crate) async fn counts(
    client: &mut (impl GenericClient + Sync),
    config: &SharedConfig,
) -> Result<Counts, AppError> {
    let row = client
        .query_one(
            "SELECT (SELECT count(*) FROM conversations WHERE node_id=$1), count(*),
         count(*) FILTER (WHERE projection <> ''),
         count(*) FILTER (WHERE projection <> '' AND generation=$2 AND array_length(vector,1)=384)
         FROM messages WHERE node_id=$1",
            &[&config.node_id, &semantic::SHARED_EMBEDDING_GENERATION],
        )
        .await
        .map_err(pg_error)?;
    Ok(Counts {
        conversations: row.try_get::<_, i64>(0).map_err(pg_error)?.cast_unsigned(),
        messages: row.try_get::<_, i64>(1).map_err(pg_error)?.cast_unsigned(),
        searchable_messages: row.try_get::<_, i64>(2).map_err(pg_error)?.cast_unsigned(),
        embeddings: row.try_get::<_, i64>(3).map_err(pg_error)?.cast_unsigned(),
    })
}

pub(crate) async fn status(config: &SharedConfig) -> Result<Counts, AppError> {
    let (mut client, _) = connection(config).await?;
    validate_version(&mut client).await?;
    counts(&mut client, config).await
}

pub(crate) async fn forget(config: &SharedConfig, id: &str) -> Result<bool, AppError> {
    let (mut client, _) = connection(config).await?;
    initialize(&mut client, config).await?;
    let mut transaction = client.transaction().await.map_err(pg_error)?;
    lock_node(&mut transaction, config, None).await?;
    let forgotten = forget_in_transaction(&mut transaction, config, id).await?;
    transaction
        .execute(
            "UPDATE nodes SET revision=revision+1 WHERE node_id=$1",
            &[&config.node_id],
        )
        .await
        .map_err(pg_error)?;
    transaction.commit().await.map_err(pg_error)?;
    Ok(forgotten)
}

pub(crate) async fn flush_forgets(
    config: &SharedConfig,
    owner: &DirectOwner,
) -> Result<bool, AppError> {
    let (mut client, _) = connection(config).await?;
    initialize(&mut client, config).await?;
    let mut transaction = client.transaction().await.map_err(pg_error)?;
    lock_node(&mut transaction, config, Some(owner)).await?;
    let mut forgotten = false;
    for id in &owner.forgets {
        forgotten |= forget_in_transaction(&mut transaction, config, id).await?;
    }
    transaction
        .execute(
            "UPDATE nodes SET revision=$2 WHERE node_id=$1",
            &[&config.node_id, &owner.revision],
        )
        .await
        .map_err(pg_error)?;
    transaction.commit().await.map_err(pg_error)?;
    Ok(forgotten)
}

async fn forget_in_transaction(
    transaction: &mut Transaction<'_>,
    config: &SharedConfig,
    id: &str,
) -> Result<bool, AppError> {
    let deleted = transaction
        .query_opt(
            "DELETE FROM conversations WHERE node_id=$1 AND id=$2 RETURNING provider",
            &[&config.node_id, &id],
        )
        .await
        .map_err(pg_error)?;
    let forgotten = deleted.is_some();
    if let Some(row) = deleted {
        let provider: String = row.try_get(0).map_err(pg_error)?;
        transaction
            .execute(
                "INSERT INTO tombstones VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
                &[&config.node_id, &provider, &id],
            )
            .await
            .map_err(pg_error)?;
    } else {
        // An offline forget can precede the very first upload of this source.
        // Preserve the intent even though PostgreSQL cannot yet identify its provider.
        transaction.execute(
            "INSERT INTO tombstones SELECT $1,provider,$2 FROM (VALUES ('claude-code'),('codex')) AS p(provider)
             ON CONFLICT DO NOTHING", &[&config.node_id, &id],
        ).await.map_err(pg_error)?;
    }
    Ok(forgotten)
}

pub(crate) const fn publication() -> Publication {
    Publication::published()
}
