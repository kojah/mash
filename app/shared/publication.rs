//! Durable publication state, routing ownership and streaming uploads.
use super::client::{Endpoint, request, response};
use super::protocol::{self, Event, Start};
use super::{Publication, PublicationState, routed};
use crate::{AppError, config::SharedConfig, storage::Storage};
use std::{io::Write, path::Path, time::Duration};
mod snapshot;

pub(crate) struct Inference<'a> {
    pub(crate) models_dir: &'a Path,
    pub(crate) embeddings: Option<&'a crate::semantic::remote::Configuration>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct SnapshotWork {
    reused_threads: u64,
    reused_embeddings: u64,
    forwarded_embeddings: u64,
    pub(super) model_inferences: u64,
    model_load_milliseconds: u64,
    embedding_milliseconds: u64,
    preparation_milliseconds: u64,
    pub(super) upload_bytes: u64,
}

pub(super) fn target(config: &SharedConfig) -> String {
    blake3::hash(
        format!(
            "attachments-v1-redaction-v2\0{}\0{}\0{}",
            config.group_id,
            config.node_id,
            config.api_url.trim_end_matches('/')
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string()
}
pub(crate) fn write_event(file: &mut impl Write, event: &Event) -> Result<(), AppError> {
    let mut safe;
    let event = if let Event::Batch(batch) = event {
        let mut batch = batch.clone();
        crate::redaction::batch(&mut batch);
        safe = Event::Batch(batch);
        &mut safe
    } else {
        event
    };
    serde_json::to_writer(&mut *file, event)
        .map_err(|_| AppError::internal("serialize index upload"))?;
    file.write_all(b"\n")
        .map_err(|_| AppError::internal("write index upload"))
}
pub(super) async fn upload(
    config: &SharedConfig,
    file: &tempfile::NamedTempFile,
) -> Result<protocol::Indexed, AppError> {
    upload_to(config, file, Endpoint::Index).await
}

pub(super) async fn upload_to(
    config: &SharedConfig,
    file: &tempfile::NamedTempFile,
    endpoint: Endpoint,
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
        request(config, reqwest::Method::POST, endpoint)?
            .header(reqwest::header::CONTENT_TYPE, "application/x-ndjson")
            .body(reqwest::Body::wrap_stream(stream)),
        endpoint,
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
                crate::progress::emit(&crate::progress::Event {
                    uploaded_bytes:Some(uploaded_bytes), total_bytes:Some(total_bytes),
                    ..crate::progress::Event::new(if committing {crate::progress::Phase::SharedCommit} else {crate::progress::Phase::SharedUpload}, elapsed, eta)
                });
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
        preparation: None,
        state: if pending {
            PublicationState::Pending
        } else {
            PublicationState::Published
        },
        error: None,
    }))
}

pub(crate) async fn publish(
    path: &Path,
    config: &SharedConfig,
    inference: Option<Inference<'_>>,
) -> Publication {
    match publish_result(path, config, inference).await {
        Ok(preparation) => Publication {
            preparation: Some(preparation),
            ..Publication::published()
        },
        Err(error) => Publication {
            preparation: None,
            state: PublicationState::Pending,
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

fn routed_scope(config: &SharedConfig) -> String {
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
    options: &crate::silver::IndexOptions,
    full: bool,
) -> Publication {
    let attempt = async {
        let config = configuration.shared.as_ref().ok_or_else(|| {
            AppError::configuration("shared destination requires shared configuration")
        })?;
        let scope = routed_scope(config);
        let db = path.to_path_buf();
        let target = routed_target(configuration)?;
        let key = target.clone();
        let (owner, policy_changed) = crate::blocking(move || {
            let storage = Storage::open_existing(&db)?;
            let changed = !storage.publication_target_matches(&key)?;
            Ok((storage.routed_owner(&scope)?, changed))
        })
        .await?;
        routed::index(
            config,
            path,
            models_dir,
            configuration.embeddings.as_ref(),
            options,
            full || policy_changed,
            &owner,
        )
        .await?;
        let db = path.to_path_buf();
        crate::blocking(move || {
            Storage::open_existing(&db)?.acknowledge_routed(&target, &owner, true)
        })
        .await?;
        Ok::<_, AppError>(())
    }
    .await;
    match attempt {
        Ok(()) => Publication::published(),
        Err(error) => Publication {
            preparation: None,
            state: PublicationState::Pending,
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
        storage.routed_owner(&scope)
    })
    .await?;
    let attempt = async {
        let target = routed_target(configuration)?;
        let forgotten = routed::flush_forgets(config, &owner).await?;
        let db = path.to_path_buf();
        crate::blocking(move || {
            Storage::open_existing(&db)?.acknowledge_routed(&target, &owner, false)
        })
        .await?;
        Ok::<_, AppError>(forgotten)
    }
    .await;
    Ok(match attempt {
        Ok(forgotten) => (Publication::published(), forgotten),
        Err(error) => (
            Publication {
                preparation: None,
                state: PublicationState::Pending,
                error: Some(error.error.message),
            },
            false,
        ),
    })
}

async fn publish_result(
    path: &Path,
    config: &SharedConfig,
    inference: Option<Inference<'_>>,
) -> Result<SnapshotWork, AppError> {
    let started = std::time::Instant::now();
    let db = path.to_path_buf();
    let key = target(config);
    crate::blocking(move || Storage::open_existing(&db)?.prepare_publication_target(&key)).await?;
    let target: super::reuse::Target =
        super::client::get(config, Endpoint::PublicationPlan).await?;
    target.validate()?;
    if let Some(revision) = already_published(path, config, &target).await? {
        acknowledge(path, config, revision).await?;
        return Ok(SnapshotWork {
            preparation_milliseconds: crate::progress::milliseconds(started),
            ..SnapshotWork::default()
        });
    }
    if target.build != crate::gold::config::Build::default() {
        let (file, revision, preparation) =
            snapshot::prepare(path, &target, inference, started).await?;
        upload(config, &file).await?;
        acknowledge(path, config, revision).await?;
        return Ok(preparation);
    }
    // Parse and spool before starting a remote transaction. Temporary files contain
    // only the outgoing data, have restrictive permissions, and are deleted on drop.
    let db = path.to_path_buf();
    let (file, revision, forwarded_embeddings) = crate::blocking(move || {
        crate::progress::during(crate::progress::Phase::SharedSnapshot, || {
            let mut file = tempfile::NamedTempFile::new()
                .map_err(|_| AppError::internal("create index upload"))?;
            let mut writer = std::io::BufWriter::with_capacity(256 * 1024, file.as_file_mut());
            let mut forwarded_embeddings = 0;
            let revision = Storage::open_catalog(&db)?.with_publication_snapshot(|snapshot| {
                snapshot::validate_owner(snapshot, &target)?;
                write_event(
                    &mut writer,
                    &Event::Start(Start {
                        reuse_gold: false,
                        expected_version: Some(target.publication_version.clone()),
                        gold_build: Some(target.build.clone()),
                        source_silver: Some(snapshot.source_silver.clone()),
                        instance: snapshot.instance.clone(),
                        revision: Some(snapshot.revision),
                        base_revision: None,
                        snapshot: true,
                        forgets: Vec::new(),
                    }),
                )?;
                for kind in [
                    protocol::BatchKind::Tombstones,
                    protocol::BatchKind::Conversations,
                    protocol::BatchKind::Messages,
                ] {
                    snapshot.visit(kind, |batch| {
                        if let protocol::Batch::Messages { records } = &batch {
                            forwarded_embeddings +=
                                records.iter().filter(|m| m.vector.is_some()).count() as u64;
                        }
                        write_event(&mut writer, &Event::Batch(batch))
                    })?;
                }
                write_event(&mut writer, &Event::Finish)
            })?;
            writer
                .flush()
                .map_err(|_| AppError::internal("flush index upload"))?;
            drop(writer);
            Ok((file, revision, forwarded_embeddings))
        })
    })
    .await?;
    let preparation = SnapshotWork {
        reused_threads: 0,
        reused_embeddings: 0,
        forwarded_embeddings,
        model_inferences: 0,
        model_load_milliseconds: 0,
        embedding_milliseconds: 0,
        preparation_milliseconds: crate::progress::milliseconds(started),
        upload_bytes: file.as_file().metadata().map_err(AppError::io)?.len(),
    };
    upload(config, &file).await?;
    acknowledge(path, config, revision).await?;
    Ok(preparation)
}

async fn acknowledge(path: &Path, config: &SharedConfig, revision: i64) -> Result<(), AppError> {
    let db = path.to_path_buf();
    let target = target(config);
    crate::blocking(move || Storage::open_existing(&db)?.acknowledge_publication(&target, revision))
        .await
}

/// Recover a lost acknowledgement without reading or uploading canonical bodies.
/// Bracket readiness with the immutable publication head: a concurrent rebuild,
/// forget, or replacement must fall back to the normal conditional publication.
async fn already_published(
    path: &Path,
    config: &SharedConfig,
    target: &super::reuse::Target,
) -> Result<Option<i64>, AppError> {
    let db = path.to_path_buf();
    let instance = target.instance.clone();
    let revision = target.publication_revision;
    let same_source = crate::blocking(move || {
        let mut same = false;
        Storage::open_catalog(&db)?.with_publication_snapshot(|snapshot| {
            same = instance.as_deref() == Some(snapshot.instance.as_str())
                && revision == snapshot.revision
                && snapshot.source_silver.is_complete()
                && snapshot.source_silver.interpretation_version
                    == crate::silver::certificate::INTERPRETATION_VERSION;
            Ok(())
        })?;
        Ok(same)
    })
    .await?;
    if !same_source {
        return Ok(None);
    }
    let freshness: super::Freshness = super::client::get(config, Endpoint::Freshness).await?;
    if !published_artifacts_current(&freshness, config, target) {
        return Ok(None);
    }
    let confirmed: super::reuse::Target =
        super::client::get(config, Endpoint::PublicationPlan).await?;
    confirmed.validate()?;
    Ok((confirmed == *target).then_some(revision))
}

fn published_artifacts_current(
    freshness: &super::Freshness,
    config: &SharedConfig,
    target: &super::reuse::Target,
) -> bool {
    freshness.group_id == config.group_id
        && freshness.group_gold.as_ref().is_some_and(|group| {
            group.build == target.build
                && group.generation == target.generation
                && group.dimensions == target.dimensions
        })
        && freshness.nodes.iter().any(|node| {
            node.node_id == config.node_id
                && node.revision == target.publication_revision
                && node.lexical_current
                && node.gold_current
                && node
                    .lexical_manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.matches(node.silver_revision.as_ref()))
                && node.gold_manifest.as_ref().is_some_and(|manifest| {
                    manifest.matches(
                        &target.build,
                        &target.generation,
                        node.silver_revision.as_ref(),
                    )
                })
        })
}
