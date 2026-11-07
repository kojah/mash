//! Durable publication state and streaming uploads.
use super::client::Endpoint;
use super::protocol::{self, Event};
use super::{Publication, PublicationState};
use crate::{AppError, config::SharedConfig, storage::Storage};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
mod snapshot;

#[derive(Clone, Copy)]
pub(crate) struct Inference<'a> {
    pub(crate) models_dir: &'a Path,
    pub(crate) local: Option<&'a crate::config::ResolvedConfig>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct SnapshotWork {
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
            "providers-six-v1-attachments-v1-redaction-v2\0{}\0{}\0{}{}",
            config.group_id,
            config.node_id,
            config.api_url.trim_end_matches('/'),
            // Changing the masking replaces everything previously published.
            if config.publication_redaction.enabled() {
                ""
            } else {
                "\0unmasked"
            }
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string()
}
pub(crate) fn write_event(
    file: &mut impl Write,
    event: &Event,
    redaction: crate::redaction::Policy,
) -> Result<(), AppError> {
    let mut safe;
    let event = if let Event::Batch(batch) = event {
        let mut batch = batch.clone();
        redaction.batch(&mut batch);
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
    super::sync::client::upload(config, file).await
}

/// One deferred publication request. Jobs are coalesced per database so a burst
/// of watch passes sends the newest snapshot only.
struct Job {
    database: std::path::PathBuf,
    config: SharedConfig,
    models: std::path::PathBuf,
    attachments: bool,
    local: Option<crate::config::ResolvedConfig>,
}

static PUBLISHER: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<Job>> =
    std::sync::OnceLock::new();

fn publisher() -> &'static tokio::sync::mpsc::UnboundedSender<Job> {
    PUBLISHER.get_or_init(|| {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Job>();
        tokio::spawn(async move {
            let mut pending: std::collections::BTreeMap<std::path::PathBuf, Job> =
                std::collections::BTreeMap::new();
            while let Some(job) = receiver.recv().await {
                pending.insert(job.database.clone(), job);
                // Collapse everything already queued: a snapshot publication
                // supersedes any older one for the same database.
                while let Ok(next) = receiver.try_recv() {
                    pending.insert(next.database.clone(), next);
                }
                for (_, job) in std::mem::take(&mut pending) {
                    let Job {
                        database,
                        config,
                        models,
                        attachments,
                        local,
                    } = job;
                    let inference = Inference {
                        models_dir: &models,
                        local: local.as_ref(),
                    };
                    let published = publish(&database, &config, Some(inference)).await;
                    if published.is_pending() {
                        tracing::warn!(
                            message = published.error.as_deref().unwrap_or("publication pending"),
                            "background publication is still pending"
                        );
                        continue;
                    }
                    if attachments
                        && let Err(error) = super::attachments::publish(&database, &config).await
                    {
                        tracing::warn!(
                            message = %error.error.message,
                            "background attachment publication failed"
                        );
                    }
                }
            }
        });
        sender
    })
}

/// Queue a publication for the process-wide publisher task. The caller returns
/// immediately; the local indexing task never waits for the network.
pub(crate) fn enqueue(
    database: &Path,
    config: &SharedConfig,
    models: &Path,
    attachments: bool,
    local: Option<&crate::config::ResolvedConfig>,
) -> Result<(), AppError> {
    publisher()
        .send(Job {
            database: database.to_path_buf(),
            config: config.clone(),
            models: models.to_path_buf(),
            attachments,
            local: local.cloned(),
        })
        .map_err(|_| AppError::internal("background publisher is unavailable"))
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

/// One publisher owns a database/destination pair at a time. A second process
/// may observe the owner within this budget, then reports shared-pending.
const PUBLISH_LOCK_WAIT: Duration = Duration::from_secs(30);
const PUBLISH_LOCK_POLL: Duration = Duration::from_millis(50);
/// Why a delta left the node's views uncertified; the publisher resupplies.
const VIEWS_INCOMPLETE: &str =
    "shared views still lack chats this delta did not change; resupplying every chat";
/// The backfill identity publishers choose for themselves; see `publish_backfill`.
const AUTO_BACKFILL: &str = "auto";
const SHARED_PENDING: &str =
    "another process is publishing this database and destination; retry when it finishes";

/// Test-only crash injection. Debug builds abort at a named point so an
/// integration test can rerun the command and prove recovery converges.
/// Release builds ignore the variable entirely.
#[cfg(debug_assertions)]
fn crash_point(name: &str) {
    if std::env::var("MASH_TEST_CRASH_AT").is_ok_and(|value| value == name) {
        std::process::abort();
    }
}

#[cfg(not(debug_assertions))]
fn crash_point(_name: &str) {}

/// Histories at least this large publish as a baseline when the node has
/// nothing on the server yet: the first batches become searchable while the
/// rest uploads.
const AUTO_BACKFILL_MIN_CHATS: u64 = 200;

fn auto_backfill_threshold() -> u64 {
    #[cfg(debug_assertions)]
    if let Ok(value) = std::env::var("MASH_TEST_AUTO_BACKFILL_MIN")
        && let Ok(value) = value.parse()
    {
        return value;
    }
    AUTO_BACKFILL_MIN_CHATS
}

/// Whether this publication should be a progressive baseline: a large local
/// history with nothing published for this node yet. Establishised nodes keep
/// replace/delta semantics so deletions still propagate.
async fn should_auto_backfill(path: &Path, config: &SharedConfig) -> Result<bool, AppError> {
    let db = path.to_path_buf();
    let counts = crate::blocking(move || Storage::open_existing(&db)?.counts()).await?;
    if counts.chats < auto_backfill_threshold() {
        return Ok(false);
    }
    let freshness: super::Freshness = super::client::get(config, Endpoint::Freshness).await?;
    Ok(!freshness
        .nodes
        .iter()
        .any(|node| node.node_id == config.node_id && node.revision > 0))
}

pub(crate) async fn publish(
    path: &Path,
    config: &SharedConfig,
    inference: Option<Inference<'_>>,
) -> Publication {
    // A failed probe must not block publication; fall back to the normal
    // strategy and let it report the actionable error.
    if let Ok(true) = should_auto_backfill(path, config).await {
        return publish_baseline_now(path, config, AUTO_BACKFILL, inference).await;
    }
    let publication = publish_with_wait(path, config, inference, PUBLISH_LOCK_WAIT).await;
    // One atomic publication cannot exceed the decoded-size limit; a node that
    // fell far behind or holds very long chats catches up through the
    // resumable, size-bounded backfill instead of staying stale.
    if matches!(
        publication.error.as_deref(),
        Some(super::sync::wire::BATCH_TOO_LARGE | VIEWS_INCOMPLETE)
    ) {
        return publish_baseline_now(path, config, AUTO_BACKFILL, inference).await;
    }
    publication
}

async fn publish_baseline_now(
    path: &Path,
    config: &SharedConfig,
    id: &str,
    inference: Option<Inference<'_>>,
) -> Publication {
    match publish_backfill(path, config, id, inference).await {
        Ok(_) => Publication::published(),
        Err(error) => {
            let mut pending = Publication::pending();
            pending.attachment_failure(error);
            pending
        }
    }
}

/// Publisher with an explicit ownership wait, used by tests.
pub(super) async fn publish_with_wait(
    path: &Path,
    config: &SharedConfig,
    inference: Option<Inference<'_>>,
    wait: Duration,
) -> Publication {
    let target = target(config);
    let lock = match acquire_publish_lock(path, &target, wait).await {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            return Publication {
                preparation: None,
                state: PublicationState::Pending,
                error: Some(SHARED_PENDING.to_owned()),
            };
        }
        Err(error) => {
            return Publication {
                preparation: None,
                state: PublicationState::Pending,
                error: Some(error.error.message),
            };
        }
    };
    let result = publish_result(path, config, inference).await;
    drop(lock);
    match result {
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

/// Lock file beside the database, scoped to the destination identity. A
/// destination change publishes a different snapshot, so it gets its own lock.
pub(super) fn publish_lock_path(database: &Path, target: &str) -> PathBuf {
    let mut lock = database.to_path_buf().into_os_string();
    lock.push(format!(".publish-{}.lock", &target[..target.len().min(16)]));
    PathBuf::from(lock)
}

fn try_publish_lock(path: &Path) -> Result<Option<std::fs::File>, AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::io)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(AppError::io)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(AppError::io(error)),
    }
}

/// Acquire the publisher lock, waiting up to `wait` for a live owner to finish.
async fn acquire_publish_lock(
    database: &Path,
    target: &str,
    wait: Duration,
) -> Result<Option<std::fs::File>, AppError> {
    let path = publish_lock_path(database, target);
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let probe = path.clone();
        if let Some(file) = crate::blocking(move || try_publish_lock(&probe)).await? {
            return Ok(Some(file));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(PUBLISH_LOCK_POLL).await;
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "keep the consistent SQLite capture, bounded spool and acknowledgement together"
)]
async fn publish_result(
    path: &Path,
    config: &SharedConfig,
    inference: Option<Inference<'_>>,
) -> Result<SnapshotWork, AppError> {
    let started = std::time::Instant::now();
    let db = path.to_path_buf();
    let key = target(config);
    crate::blocking(move || Storage::open_existing(&db)?.prepare_publication_target(&key)).await?;
    let freshness: super::Freshness = super::client::get(config, Endpoint::Freshness).await?;
    let target = super::reuse::Target::from_freshness(&freshness, config)?;
    if let Some(revision) = already_published(path, config, &target, &freshness).await? {
        acknowledge(path, config, revision).await?;
        return Ok(SnapshotWork {
            preparation_milliseconds: crate::progress::milliseconds(started),
            ..SnapshotWork::default()
        });
    }
    // Changed recipes/models require complete replacement, even with a journal.
    let allow_delta = target.publication_revision > 0 && delta_applies(&freshness, config, &target);
    let selection = if allow_delta {
        snapshot::Selection::Delta {
            journal_base: local_base(path, &self::target(config), target.publication_revision)
                .unwrap_or(target.publication_revision),
        }
    } else {
        snapshot::Selection::Replace
    };
    let (file, revision, preparation) =
        snapshot::prepare(path, &target, inference, started, selection).await?;
    let indexed = upload(config, &file).await?;
    crash_point("after_publish_before_ack");
    acknowledge(path, config, revision).await?;
    record_server_base(path, &self::target(config), revision, indexed.revision);
    // A delta supplies only changed chats; if the node's views still lack
    // others, nothing later would supply them, so resupply everything once.
    if allow_delta && !indexed.views_current {
        return Err(AppError::shared(VIEWS_INCOMPLETE));
    }
    Ok(preparation)
}

async fn acknowledge(path: &Path, config: &SharedConfig, revision: i64) -> Result<(), AppError> {
    acknowledge_with(
        path,
        config,
        revision,
        crate::storage::publication::JournalCoverage::Incremental { through: revision },
    )
    .await
}

async fn acknowledge_with(
    path: &Path,
    config: &SharedConfig,
    revision: i64,
    coverage: crate::storage::publication::JournalCoverage,
) -> Result<(), AppError> {
    let db = path.to_path_buf();
    let target = target(config);
    crate::blocking(move || {
        Storage::open_existing(&db)?.acknowledge_publication(&target, revision, coverage)
    })
    .await
}

/// Durable per-batch progress for one baseline, so a restart resumes instead of
/// re-sending every batch.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct BackfillReceipts {
    backfill_id: String,
    source_revision: i64,
    server_revision: i64,
    completed: std::collections::BTreeSet<String>,
}

impl BackfillReceipts {
    fn record(&mut self, digest: &str, server_revision: i64) {
        self.completed.insert(digest.to_owned());
        // Concurrent lanes can report committed revisions out of order.
        self.server_revision = self.server_revision.max(server_revision);
    }
}

/// Which server revision the last acknowledged local revision became. Ordinary
/// publications keep the client's revision, but a backfill's server numbers its
/// batches from its own head; deltas then need both to line up again.
#[derive(serde::Deserialize, serde::Serialize)]
struct ServerBase {
    target: String,
    local: i64,
    server: i64,
}

fn server_base_path(database: &Path) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(".publication-base.json");
    PathBuf::from(path)
}

/// Best effort: without the record the next publication is a full snapshot.
fn record_server_base(database: &Path, target: &str, local: i64, server: i64) {
    let path = server_base_path(database);
    let record = ServerBase {
        target: target.to_owned(),
        local,
        server,
    };
    let Ok(bytes) = serde_json::to_vec(&record) else {
        return;
    };
    let temporary = path.with_extension("json.tmp");
    if std::fs::write(&temporary, bytes).is_ok() {
        let _ = std::fs::rename(&temporary, &path);
    }
}

/// The local revision acknowledged when the server reached `server`, if that
/// is still this target's last record; otherwise the revisions coincide.
fn local_base(database: &Path, target: &str, server: i64) -> Option<i64> {
    let record: ServerBase =
        serde_json::from_slice(&std::fs::read(server_base_path(database)).ok()?).ok()?;
    (record.target == target && record.server == server).then_some(record.local)
}

fn backfill_receipts_path(database: &Path) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(".backfill.json");
    PathBuf::from(path)
}

fn load_backfill_receipts(path: &Path, backfill_id: &str, revision: i64) -> BackfillReceipts {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BackfillReceipts>(&bytes).ok())
        .filter(|receipts| {
            receipts.backfill_id == backfill_id && receipts.source_revision == revision
        })
        .unwrap_or_else(|| BackfillReceipts {
            backfill_id: backfill_id.to_owned(),
            source_revision: revision,
            server_revision: 0,
            completed: std::collections::BTreeSet::new(),
        })
}

fn store_backfill_receipts(path: &Path, receipts: &BackfillReceipts) -> Result<(), AppError> {
    let temporary = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec(receipts).map_err(|_| AppError::internal("encode backfill receipts"))?;
    std::fs::write(&temporary, bytes).map_err(AppError::io)?;
    std::fs::rename(&temporary, path).map_err(AppError::io)
}

/// Publish one frozen baseline progressively through `/v1/sync` and complete
/// it. This is the operator entry point behind `mash backfill`.
pub(crate) async fn publish_backfill(
    path: &Path,
    config: &SharedConfig,
    backfill_id: &str,
    inference: Option<Inference<'_>>,
) -> Result<i64, AppError> {
    if backfill_id.is_empty() || backfill_id.len() > 4096 {
        return Err(AppError::usage("backfill identity is required"));
    }
    let requested = backfill_id;
    let target_key = target(config);
    let _lock = acquire_publish_lock(path, &target_key, PUBLISH_LOCK_WAIT)
        .await?
        .ok_or_else(|| AppError::shared(SHARED_PENDING))?;
    let freshness: super::Freshness = super::client::get(config, Endpoint::Freshness).await?;
    let target = super::reuse::Target::from_freshness(&freshness, config)?;
    if target.build != crate::views::config::Build::default() {
        return Err(AppError::usage(
            "backfill requires the default views recipe; publish normally to switch recipes",
        ));
    }
    let (file, revision, _) = snapshot::prepare(
        path,
        &target,
        inference,
        std::time::Instant::now(),
        snapshot::Selection::Backfill,
    )
    .await?;
    // An identity names one baseline: automatic catch-ups take the revision
    // they freeze, so a later one is new while a retry of this one resumes.
    let requested = if requested == AUTO_BACKFILL {
        format!("{AUTO_BACKFILL}-{revision}")
    } else {
        requested.to_owned()
    };
    // The transfer catalog is service-wide but backfills are looked up per
    // (group, node). An unscoped identity would collide with another group's
    // baseline and be rejected as unknown.
    let backfill_id = &destination_scoped(config, &requested);
    let plan = super::sync::client::plan_backfill_file(
        &file,
        super::sync::client::BackfillLimits::default(),
    )?;
    let receipts_path = backfill_receipts_path(path);
    let mut receipts = load_backfill_receipts(&receipts_path, backfill_id, revision);
    let completed = receipts.completed.clone();
    let resume = super::sync::client::BackfillResume {
        completed: &completed,
        last_revision: receipts.server_revision,
    };
    let server_revision = super::sync::client::upload_backfill_plan_with_config(
        config,
        &plan,
        backfill_id,
        resume,
        |digest, revision| {
            receipts.record(digest, revision);
            store_backfill_receipts(&receipts_path, &receipts)
        },
    )
    .await?;
    let server_revision = finish_record_backfill(config, &plan, server_revision).await?;
    // Every batch is committed and covered; progress no longer needs retaining.
    let _ = std::fs::remove_file(&receipts_path);
    acknowledge_with(
        path,
        config,
        revision,
        crate::storage::publication::JournalCoverage::BaselineComplete { through: revision },
    )
    .await?;
    // The server renumbers baseline batches from its own head, so its revision
    // now differs from the local one deltas are journaled against.
    record_server_base(path, &target_key, revision, server_revision);
    Ok(server_revision)
}

/// Records follow the complete chat baseline, so their citations and links can
/// be validated in one transaction. The deterministic envelope is retryable
/// through the ordinary durable synchronization receipt.
async fn finish_record_backfill(
    config: &SharedConfig,
    plan: &super::sync::client::BackfillPlan,
    server_revision: i64,
) -> Result<i64, AppError> {
    if plan.record_events.is_empty() {
        return Ok(server_revision);
    }
    let revision = server_revision
        .checked_add(1)
        .ok_or_else(|| AppError::usage("publication revision exhausted"))?;
    let mut file = tempfile::NamedTempFile::new().map_err(AppError::io)?;
    write_event(
        &mut file,
        &Event::Start(protocol::Start {
            views_build: plan.start.views_build.clone(),
            source_silver: plan.start.source_silver.clone(),
            instance: plan.start.instance.clone(),
            revision: Some(revision),
            base_revision: Some(server_revision),
            snapshot: false,
            forgets: Vec::new(),
        }),
        config.publication_redaction,
    )?;
    for event in &plan.record_events {
        write_event(&mut file, event, config.publication_redaction)?;
    }
    write_event(&mut file, &Event::Finish, config.publication_redaction)?;
    file.flush().map_err(AppError::io)?;
    Ok(upload(config, &file).await?.revision)
}

/// Backfill identity qualified by the destination it belongs to.
fn destination_scoped(config: &SharedConfig, id: &str) -> String {
    format!("[{}][{}]{id}", config.group_id, config.node_id)
}

/// Spool the node's complete history as a non-snapshot batch frozen at the next
/// local revision. The server merges it with per-chat ordering.
/// Recover a lost acknowledgement without reading or uploading canonical bodies.
/// Readiness is read twice around the local check: a concurrent publication,
/// forget or settings change falls back to a normal publication.
async fn already_published(
    path: &Path,
    config: &SharedConfig,
    target: &super::reuse::Target,
    freshness: &super::Freshness,
) -> Result<Option<i64>, AppError> {
    let db = path.to_path_buf();
    let instance = target.instance.clone();
    let revision = target.publication_revision;
    let same_source = crate::blocking(move || {
        let mut same = false;
        Storage::open_silver(&db)?.with_publication_snapshot(|snapshot| {
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
    if !published_artifacts_current(freshness, config, target) {
        return Ok(None);
    }
    let confirmed: super::Freshness = super::client::get(config, Endpoint::Freshness).await?;
    Ok(
        (super::reuse::Target::from_freshness(&confirmed, config)? == *target
            && published_artifacts_current(&confirmed, config, target))
        .then_some(revision),
    )
}

/// Whether the server holds exactly this node's last acknowledged revision
/// under the same views recipe, so a delta can follow it. Unlike
/// `published_artifacts_current`, the node's views need not be certified: the
/// server merges a delta into the views it has and certifies only when every
/// chat is covered. Requiring certification instead made a node that missed
/// one chat (say, one still growing during a long backfill) resend everything
/// on every publication.
fn delta_applies(
    freshness: &super::Freshness,
    config: &SharedConfig,
    target: &super::reuse::Target,
) -> bool {
    freshness.group_id == config.group_id
        && freshness.group_views.as_ref().is_some_and(|group| {
            group.build == target.build
                && group.generation == target.generation
                && group.dimensions == target.dimensions
        })
        && freshness.nodes.iter().any(|node| {
            node.node_id == config.node_id && node.revision == target.publication_revision
        })
}

fn published_artifacts_current(
    freshness: &super::Freshness,
    config: &SharedConfig,
    target: &super::reuse::Target,
) -> bool {
    freshness.group_id == config.group_id
        && freshness.group_views.as_ref().is_some_and(|group| {
            group.build == target.build
                && group.generation == target.generation
                && group.dimensions == target.dimensions
        })
        && freshness.nodes.iter().any(|node| {
            node.node_id == config.node_id
                && node.revision == target.publication_revision
                && node.lexical_current
                && node.views_current
                && node
                    .lexical_manifest
                    .as_ref()
                    .is_some_and(|manifest| manifest.matches(node.silver_revision.as_ref()))
                && node.views_manifest.as_ref().is_some_and(|manifest| {
                    manifest.matches(
                        &target.build,
                        &target.generation,
                        node.silver_revision.as_ref(),
                    )
                })
        })
}

#[cfg(test)]
mod server_base_tests {
    use super::{load_backfill_receipts, local_base, record_server_base, store_backfill_receipts};

    #[test]
    fn backfill_receipts_keep_source_identity_and_server_progress_separate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backfill.json");
        let mut receipts = load_backfill_receipts(&path, "baseline", 5_140_005);
        assert_eq!(receipts.server_revision, 0);
        receipts.record("first", 3_528_522);
        store_backfill_receipts(&path, &receipts).unwrap();

        // Reopen after interruption: a server receipt must not change the
        // source revision used to recognize the frozen baseline.
        let mut resumed = load_backfill_receipts(&path, "baseline", 5_140_005);
        assert!(resumed.completed.contains("first"));
        assert_eq!(resumed.server_revision, 3_528_522);
        resumed.record("later", 3_528_524);
        resumed.record("earlier", 3_528_523);
        store_backfill_receipts(&path, &resumed).unwrap();
        let resumed = load_backfill_receipts(&path, "baseline", 5_140_005);
        assert_eq!(resumed.source_revision, 5_140_005);
        assert_eq!(resumed.server_revision, 3_528_524);
        assert_eq!(resumed.completed.len(), 3);
        for (id, revision) in [("other", 5_140_005), ("baseline", 5_140_006)] {
            let fresh = load_backfill_receipts(&path, id, revision);
            assert!(fresh.completed.is_empty());
            assert_eq!(fresh.server_revision, 0);
        }
    }

    #[test]
    fn a_backfilled_server_revision_maps_back_to_the_local_one() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("mash.sqlite3");
        assert_eq!(local_base(&database, "target", 7), None);
        record_server_base(&database, "target", 5_140_005, 3_528_522);
        assert_eq!(local_base(&database, "target", 3_528_522), Some(5_140_005));
        // Another server head or destination means something else wrote there.
        assert_eq!(local_base(&database, "target", 3_528_523), None);
        assert_eq!(local_base(&database, "other", 3_528_522), None);
    }
}
