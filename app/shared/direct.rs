use super::client::{Endpoint, get, post};
use super::protocol::{
    Batch, Checkpoint, ConversationRecord, Event, Forget, Forgotten, MessageRecord, NodeState,
    Start,
};
use super::publication::{upload, write_event};
use crate::AppError;
use crate::config::SharedConfig;
use crate::semantic::{self, EmbeddingPool, EmbeddingSummary, Models};
use crate::silver::{self, Conversation, DirectScan, IndexOptions, IndexSummary};
use crate::storage::Counts;
use crate::storage::publication::DirectOwner;
use std::path::Path;
use std::time::Instant;
const DIRECT_INSTANCE: &str = "shared-only:v1";
mod claude;
#[cfg(test)]
mod tests;
pub(crate) struct IndexResult {
    pub(crate) counts: Counts,
    pub(crate) summary: IndexSummary,
    pub(crate) embeddings: EmbeddingSummary,
    pub(crate) model_load_milliseconds: u64,
    pub(crate) ingestion_milliseconds: u64,
    pub(crate) embedding_milliseconds: u64,
}

fn elapsed(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(crate) async fn index(
    config: &SharedConfig,
    database: &Path,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    options: &IndexOptions,
    full: bool,
    owner: Option<&DirectOwner>,
) -> Result<IndexResult, AppError> {
    let root = models_dir.to_path_buf();
    let remote = embeddings.cloned();
    let input = options.clone();
    let scans = crate::blocking(move || {
        Models::new(root).with_remote(remote).require_indexer()?;
        silver::direct_scans(&input)
    })
    .await?;
    let state: NodeState = get(config, Endpoint::State).await?;
    let instance = owner.map_or(DIRECT_INSTANCE, |o| o.instance.as_str());
    if state.instance.as_deref().is_some_and(|i| i != instance) {
        return Err(AppError::shared(
            "node_id belongs to another database or indexing mode; choose a new node_id",
        ));
    }
    let mut file = crate::blocking(|| {
        tempfile::NamedTempFile::new().map_err(|_| AppError::internal("create index upload"))
    })
    .await?;
    write_event(
        file.as_file_mut(),
        &Event::Start(Start {
            instance: instance.to_owned(),
            revision: owner.map(|o| o.revision),
            base_revision: Some(state.revision),
            snapshot: false,
            forgets: owner.map_or_else(Vec::new, |o| o.forgets.clone()),
        }),
    )?;
    let mut result = IndexResult {
        counts: Counts::default(),
        summary: IndexSummary::default(),
        embeddings: EmbeddingSummary::default(),
        model_load_milliseconds: 0,
        ingestion_milliseconds: 0,
        embedding_milliseconds: 0,
    };
    let ingestion_started = Instant::now();
    crate::progress::phase(crate::progress::Phase::SharedIngestion, 0);
    let mut pool = None;
    for scan in &scans {
        if scan.provider == crate::provider::Provider::ClaudeCode {
            let owner = owner.ok_or_else(|| {
                AppError::internal("routed Claude indexing requires a local owner")
            })?;
            claude::ingest(
                file.as_file_mut(),
                &state,
                owner,
                database,
                scan,
                full,
                models_dir,
                embeddings,
                &mut pool,
                &mut result,
            )
            .await?;
            continue;
        }
        ingest_scan(
            file.as_file_mut(),
            &state,
            owner,
            scan,
            full,
            models_dir,
            embeddings,
            &mut pool,
            &mut result,
        )
        .await?;
    }
    result.ingestion_milliseconds = elapsed(ingestion_started)
        .saturating_sub(result.model_load_milliseconds)
        .saturating_sub(result.embedding_milliseconds);
    write_event(file.as_file_mut(), &Event::Finish)?;
    let indexed = upload(config, &file).await?;
    result.counts = indexed.counts;
    result.summary.removed_messages += indexed.removed_messages;
    result.summary.purged_conversations += indexed.purged_conversations;
    result.summary.committed_batches = 1;
    Ok(result)
}
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "concrete source scan and bounded model work"
)]
async fn ingest_scan(
    file: &mut std::fs::File,
    state: &NodeState,
    owner: Option<&DirectOwner>,
    scan: &DirectScan,
    full: bool,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    pool: &mut Option<EmbeddingPool>,
    result: &mut IndexResult,
) -> Result<(), AppError> {
    let checkpoints: std::collections::HashMap<_, _> = state
        .sources
        .iter()
        .filter(|s| s.provider == scan.provider)
        .map(|s| (s.path_hash.as_str(), s))
        .collect();
    let mut observed = Vec::new();
    for source in &scan.sources {
        let hash = source.identity.publication_hash();
        observed.push(hash.clone());
        if observed.len() == 128 {
            write_event(
                file,
                &Event::Seen {
                    provider: scan.provider,
                    hashes: std::mem::take(&mut observed),
                },
            )?;
        }
        let identity = source.clone();
        let stamp = crate::blocking(move || identity.stamp()).await?;
        let size = stamp.size_bytes;
        let modified = stamp.activity_ns.unwrap_or(stamp.modified_ns);
        if scan.cutoff.is_some_and(|c| modified < c) {
            continue;
        }
        result.summary.scanned_files += 1;
        result.summary.processed_files += 1;
        let bytes = source
            .estimated_bytes
            .unwrap_or_else(|| u64::try_from(size).unwrap_or_default());
        result.summary.discovered_bytes += bytes;
        result.summary.processed_bytes += bytes;
        let checkpoint = checkpoints.get(hash.as_str());
        if !full
            && checkpoint.is_some_and(|c| {
                c.size_bytes == size
                    && c.modified_ns == modified
                    && c.fingerprint == stamp.fingerprint
                    && c.ready
            })
        {
            result.summary.unchanged_sources += 1;
            result.summary.checkpoint_skipped_sources += 1;
            continue;
        }
        let source = source.clone();
        let expected_stamp = stamp.clone();
        let provider = scan.provider;
        let (parsed, stamp) = crate::blocking(move || {
            DirectScan::parse_source_with_retries(provider, &source, expected_stamp)
        })
        .await?;
        let size = stamp.size_bytes;
        let modified = stamp.activity_ns.unwrap_or(stamp.modified_ns);
        if parsed.malformed_records != 0 {
            return Err(AppError::shared(
                "shared indexing encountered malformed history; upload was not committed",
            ));
        }
        let conversation_id = parsed.conversation.as_ref().map(|c| c.id.clone());
        if let Some(previous) = checkpoint
            .and_then(|c| c.conversation_id.as_ref())
            .filter(|id| Some(*id) != conversation_id.as_ref())
        {
            write_event(
                file,
                &Event::Remove {
                    id: previous.clone(),
                },
            )?;
        }
        if let Some(conversation) = &parsed.conversation {
            let forgotten = state.tombstones.iter().any(|t| {
                t.provider == conversation.provider && t.conversation_id == conversation.id
            }) || owner.is_some_and(|o| o.forgets.contains(&conversation.id));
            if forgotten {
                result.summary.tombstoned_sources += 1;
            } else {
                replace_conversation(file, conversation, models_dir, embeddings, pool, result)
                    .await?;
            }
        }
        write_event(
            file,
            &Event::Source(Checkpoint {
                provider: scan.provider,
                path_hash: hash,
                size_bytes: size,
                modified_ns: modified,
                fingerprint: stamp.fingerprint,
                conversation_id,
                ready: true,
            }),
        )?;
    }
    let verification = scan.clone();
    crate::blocking(move || verification.verify()).await?;
    if !observed.is_empty() {
        write_event(
            file,
            &Event::Seen {
                provider: scan.provider,
                hashes: observed,
            },
        )?;
    }
    if scan.complete {
        write_event(
            file,
            &Event::Reconcile {
                provider: scan.provider,
                cutoff: scan.cutoff,
            },
        )?;
    }
    Ok(())
}
async fn embed_texts(
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    pool: &mut Option<EmbeddingPool>,
    texts: &[&str],
    result: &mut IndexResult,
) -> Result<Vec<Vec<f64>>, AppError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let existing = pool.take();
    let remote = embeddings.cloned();
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
            None => crate::progress::during(crate::progress::Phase::ModelLoading, || {
                Models::new(root).with_remote(remote).load_indexer()
            })?
            .ok_or_else(|| {
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
    file: &mut std::fs::File,
    conversation: &Conversation,
    models_dir: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    pool: &mut Option<EmbeddingPool>,
    result: &mut IndexResult,
) -> Result<(), AppError> {
    let total_vectors = conversation.messages.summary()?.searchable;
    let started = Instant::now();
    let initial_vectors = result.embeddings.stored_vectors;
    let mut estimate = crate::progress::Estimate::new();
    write_event(
        file,
        &Event::Batch(Batch::Conversations {
            records: vec![ConversationRecord {
                id: conversation.id.clone(),
                provider: conversation.provider,
                title: conversation.title.clone(),
                project: conversation.project.clone(),
                repository: conversation.repository.clone(),
                created_at: conversation.created_at,
                updated_at: conversation.updated_at,
            }],
        }),
    )?;
    let mut messages = conversation.messages.rows()?;
    loop {
        let batch = messages
            .by_ref()
            .take(32)
            .collect::<Result<Vec<_>, AppError>>()?;
        if batch.is_empty() {
            break;
        }
        let texts = batch
            .iter()
            .map(|m| m.dialogue())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>();
        let mut vectors = embed_texts(models_dir, embeddings, pool, &texts, result)
            .await?
            .into_iter();
        result.embeddings.model_inferences += texts.len() as u64;
        drop(texts);
        let mut records = Vec::new();
        for message in batch {
            let message = message.into_owned();
            let projection = message.dialogue_text.as_deref().unwrap_or(&message.content);
            let vector = if projection.is_empty() {
                None
            } else {
                Some(
                    vectors
                        .next()
                        .ok_or_else(|| AppError::model("missing shared message embedding"))?,
                )
            };
            let generation = vector.as_ref().map(|_| {
                embeddings.map_or_else(
                    || semantic::SHARED_EMBEDDING_GENERATION.to_owned(),
                    |c| semantic::model_space(&c.model, c.dimensions),
                )
            });
            let has_vector = vector.is_some();
            let projection = message
                .dialogue_text
                .unwrap_or_else(|| message.content.clone());
            records.push(MessageRecord {
                id: message.id,
                conversation_id: conversation.id.clone(),
                ordinal: message.ordinal,
                context: message.context,
                role: message.role,
                content: message.content,
                projection,
                created_at: message.created_at,
                generation,
                vector,
            });
            result.summary.changed_messages += 1;
            result.embeddings.stored_vectors += u64::from(has_vector);
        }
        write_event(file, &Event::Batch(Batch::Messages { records }))?;
        let stored_vectors = result.embeddings.stored_vectors - initial_vectors;
        let elapsed_ms = elapsed(started);
        crate::progress::emit(&crate::progress::Event {
            stored_vectors: Some(stored_vectors),
            total_vectors: Some(total_vectors),
            eta_scope: crate::progress::EtaScope::Conversation,
            ..crate::progress::Event::new(
                crate::progress::Phase::SharedEmbeddings,
                elapsed_ms,
                estimate.observe(stored_vectors, total_vectors, elapsed_ms),
            )
        });
    }
    Ok(())
}
pub(crate) async fn flush_forgets(
    config: &SharedConfig,
    owner: &DirectOwner,
) -> Result<bool, AppError> {
    let result: Forgotten = post(
        config,
        Endpoint::Forget,
        &Forget {
            instance: owner.instance.clone(),
            revision: Some(owner.revision),
            ids: owner.forgets.clone(),
        },
    )
    .await?;
    Ok(result.forgotten)
}
