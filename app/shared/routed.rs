//! Coordinate independently routed silver reconciliation and shared gold building.
//! All publication uses HTTP; this module does not connect to shared databases.
use super::client::{Endpoint, get, post};
use super::protocol::{Checkpoint, Event, Forget, Forgotten, NodeState, Start};
use super::publication::{upload, write_event};
use crate::AppError;
use crate::config::SharedConfig;
use crate::semantic::{self, EmbeddingPool, Models};
use crate::silver::{self, DirectScan, IndexOptions, IndexSummary};
use crate::storage::Counts;
use crate::storage::publication::RoutedOwner;
use std::path::Path;
use std::time::Instant;
mod claude;
pub(crate) struct IndexResult {
    pub(crate) counts: Counts,
    pub(crate) summary: IndexSummary,
    pub(crate) gold: crate::gold::shared::Build,
    pub(crate) ingestion_milliseconds: u64,
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
    owner: &RoutedOwner,
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
    let instance = owner.instance.as_str();
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
            revision: Some(owner.revision),
            base_revision: Some(state.revision),
            snapshot: false,
            forgets: owner.forgets.clone(),
        }),
    )?;
    let mut result = IndexResult {
        counts: Counts::default(),
        summary: IndexSummary::default(),
        gold: crate::gold::shared::Build::default(),
        ingestion_milliseconds: 0,
    };
    let ingestion_started = Instant::now();
    crate::progress::phase(crate::progress::Phase::SharedIngestion, 0);
    let mut pool = None;
    for scan in &scans {
        if scan.provider == crate::provider::Provider::ClaudeCode {
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
    result.summary.changed_messages += result.gold.messages;
    result.ingestion_milliseconds = elapsed(ingestion_started)
        .saturating_sub(result.gold.model_load_milliseconds)
        .saturating_sub(result.gold.embedding_milliseconds);
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
    owner: &RoutedOwner,
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
            }) || owner.forgets.contains(&conversation.id);
            if forgotten {
                result.summary.tombstoned_sources += 1;
            } else {
                crate::gold::shared::materialize(
                    file,
                    conversation,
                    models_dir,
                    embeddings,
                    pool,
                    &mut result.gold,
                )
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
pub(crate) async fn flush_forgets(
    config: &SharedConfig,
    owner: &RoutedOwner,
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
