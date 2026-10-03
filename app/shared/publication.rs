//! Durable publication state, routing ownership and streaming uploads.
use super::client::{Endpoint, request, response};
use super::protocol::{self, Event, Start};
use super::{Publication, PublicationState, direct};
use crate::{AppError, config::SharedConfig, storage::Storage};
use std::{io::Write, path::Path, time::Duration};

pub(super) fn target(config: &SharedConfig) -> String {
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
pub(crate) fn write_event(file: &mut impl Write, event: &Event) -> Result<(), AppError> {
    serde_json::to_writer(&mut *file, event)
        .map_err(|_| AppError::internal("serialize index upload"))?;
    file.write_all(b"\n")
        .map_err(|_| AppError::internal("write index upload"))
}
pub(super) async fn upload(
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
        request(config, reqwest::Method::POST, Endpoint::Index)?
            .header(reqwest::header::CONTENT_TYPE, "application/x-ndjson")
            .body(reqwest::Body::wrap_stream(stream)),
        Endpoint::Index,
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
        state: if pending {
            PublicationState::Pending
        } else {
            PublicationState::Published
        },
        error: None,
    }))
}

pub(crate) async fn publish(path: &Path, config: &SharedConfig) -> Publication {
    match publish_result(path, config).await {
        Ok(()) => Publication::published(),
        Err(error) => Publication {
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
        let owner =
            crate::blocking(move || Storage::open_existing(&db)?.direct_owner(&scope)).await?;
        let target = routed_target(configuration)?;
        direct::index(
            config,
            path,
            models_dir,
            configuration.embeddings.as_ref(),
            options,
            full,
            &owner,
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
                state: PublicationState::Pending,
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
        crate::progress::during(crate::progress::Phase::SharedSnapshot, || {
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
                for kind in [
                    protocol::BatchKind::Tombstones,
                    protocol::BatchKind::Conversations,
                    protocol::BatchKind::Messages,
                ] {
                    snapshot.visit(kind, |batch| write_event(&mut writer, &Event::Batch(batch)))?;
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
