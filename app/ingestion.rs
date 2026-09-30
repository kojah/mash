use crate::provider::Provider;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant, SystemTime};

mod admission;
mod aider;
mod antigravity;
#[cfg(test)]
mod benchmarks;
pub(crate) mod claude;
mod cline;
mod copilot;
mod cursor;
mod discovery;
mod gemini;
mod hermes;
mod identities;
mod json;
pub(crate) mod messages;
mod native;
mod openclaw;
mod opencode;
mod parsers;
mod pi;
mod records;
mod source;
mod sqlite_source;
#[cfg(test)]
mod tests;
mod windsurf;

#[cfg(test)]
pub(crate) use antigravity::tests::write as write_antigravity_fixture;

#[cfg(test)]
use discovery::source_stamp;
pub(crate) use discovery::{DirectScan, SourceStamp, direct_scans, source_stamp_for};
use discovery::{
    Discovery, cutoff_modified_ns, discover_history_files, verify_authoritative_discovery,
};
pub(crate) use discovery::{validate_output_path, watch_roots};
pub(crate) use source::{HistorySource, SourceIdentity};

use crate::AppError;
use crate::storage::Storage;

const MAX_TOOL_OUTPUT_CHARS: usize = 128 * 1024;
const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
const CHECKPOINT_FILES: u64 = 32;
const CHECKPOINT_BYTES: u64 = 128 * 1024 * 1024;

impl Provider {
    fn parser(self) -> fn(&HistorySource) -> Result<ParsedFile, AppError> {
        match self {
            Self::ClaudeCode => |source| parsers::parse_claude(&source.identity.path),
            Self::Codex => |source| parsers::parse_codex(&source.identity.path),
            Self::Aider => |source| aider::parse(&source.identity.path),
            Self::Hermes => |source| hermes::parse(&source.identity),
            Self::Opencode => |source| opencode::parse(&source.identity),
            Self::Openclaw => |source| openclaw::parse(&source.identity),
            Self::Pi => |source| pi::parse(&source.identity.path),
            Self::Copilot => copilot::parse_source,
            Self::GeminiCli => |source| gemini::parse(&source.identity.path),
            Self::Cline => cline::parse_source,
            Self::Cursor => |source| cursor::parse(&source.identity),
            Self::Windsurf => |source| windsurf::parse(&source.identity.path),
            Self::Antigravity => |source| antigravity::parse(&source.identity),
        }
    }
}

#[derive(Clone)]
pub(crate) struct IndexOptions {
    pub(crate) providers: crate::provider::ProviderRoots,
    pub(crate) roots_are_authoritative: bool,
    pub(crate) since_days: Option<u32>,
}

#[derive(Default)]
pub(crate) struct IndexSummary {
    pub(crate) scanned_files: u64,
    pub(crate) malformed_records: u64,
    pub(crate) changed_messages: u64,
    pub(crate) removed_messages: u64,
    pub(crate) unchanged_sources: u64,
    pub(crate) checkpoint_skipped_sources: u64,
    pub(crate) tombstoned_sources: u64,
    pub(crate) purged_conversations: u64,
    pub(crate) committed_batches: u64,
    pub(crate) discovered_bytes: u64,
    pub(crate) processed_bytes: u64,
    pub(crate) processed_files: u64,
}

pub(crate) struct Conversation {
    pub(crate) id: String,
    pub(crate) provider: Provider,
    pub(crate) source: SourceIdentity,
    pub(crate) claude_lineage: Option<claude::Lineage>,
    pub(crate) project: Option<String>,
    pub(crate) repository: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) created_at: Option<i64>,
    pub(crate) updated_at: Option<i64>,
    pub(crate) messages: messages::Messages,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NormalizedMessage {
    pub(crate) id: String,
    pub(crate) ordinal: i64,
    pub(crate) context: crate::message_context::MessageContext,
    pub(crate) role: crate::role::Role,
    pub(crate) content: String,
    /// `None` searches canonical content, an empty string makes the message
    /// context-only, and a nonempty value searches that filtered projection.
    pub(crate) search_projection: Option<String>,
    pub(crate) created_at: Option<i64>,
}

pub(crate) struct ParsedFile {
    pub(crate) conversation: Option<Conversation>,
    pub(crate) malformed_records: u64,
}

#[derive(Clone, Copy)]
struct ProviderScan<'a> {
    provider: Provider,
    roots: &'a [PathBuf],
    cutoff_modified_ns: Option<i64>,
    parse: fn(&HistorySource) -> Result<ParsedFile, AppError>,
    authoritative: bool,
}

struct SourceFile {
    history: HistorySource,
    stamp: SourceStamp,
    estimated_bytes: u64,
}

struct ParsedSource<'a> {
    source: SourceFile,
    parsed: Result<ParsedFile, AppError>,
    // Release admission only after normalized content is applied or discarded.
    _reservation: Option<admission::Reservation<'a>>,
}

struct IndexProgress {
    started: Instant,
    last_event: Instant,
    phase_started: Instant,
    estimate: crate::progress::Estimate,
    parser_admission: Option<crate::progress::ParserAdmission>,
}

pub(crate) fn index(
    storage: &mut Storage,
    options: &IndexOptions,
) -> Result<IndexSummary, AppError> {
    index_at(storage, options, SystemTime::now())
}

fn index_at(
    storage: &mut Storage,
    options: &IndexOptions,
    run_started: SystemTime,
) -> Result<IndexSummary, AppError> {
    let mut summary = IndexSummary {
        scanned_files: 0,
        malformed_records: 0,
        changed_messages: 0,
        removed_messages: 0,
        unchanged_sources: 0,
        checkpoint_skipped_sources: 0,
        tombstoned_sources: 0,
        purged_conversations: 0,
        committed_batches: 0,
        discovered_bytes: 0,
        processed_bytes: 0,
        processed_files: 0,
    };
    let mut progress = IndexProgress::new();
    let cutoff_modified_ns = cutoff_modified_ns(run_started, options.since_days)?;

    // Inspect every configured root before opening the ingestion pipeline. A
    // bad root must fail closed without committing another provider first.
    let discoveries = options
        .providers
        .iter()
        .map(|(&provider, roots)| {
            crate::progress::during(crate::progress::Phase::SourceDiscovery, || {
                discover_history_files(provider, roots, options.roots_are_authoritative)
            })
            .map(|discovery| (provider, roots, discovery))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (provider, roots, discovery) in discoveries {
        index_provider(
            storage,
            ProviderScan {
                provider,
                roots,
                cutoff_modified_ns,
                parse: provider.parser(),
                authoritative: options.roots_are_authoritative,
            },
            discovery,
            &mut summary,
            &mut progress,
        )?;
    }

    summary.processed_files = summary.scanned_files;
    progress.emit(&summary, crate::progress::Phase::IngestionComplete, true);

    Ok(summary)
}

#[tracing::instrument(name = "ingest_provider", skip_all, fields(provider = scan.provider.as_str(), authoritative = scan.authoritative))]
fn index_provider(
    storage: &mut Storage,
    scan: ProviderScan<'_>,
    discovery: Discovery,
    summary: &mut IndexSummary,
    progress: &mut IndexProgress,
) -> Result<(), AppError> {
    if !scan.authoritative {
        return index_provider_inner(storage, scan, discovery, summary, progress);
    }
    storage.begin_provider_scan()?;
    match index_provider_inner(storage, scan, discovery, summary, progress) {
        Ok(()) => storage.finish_provider_scan(),
        Err(error) => match storage.rollback_provider_scan() {
            Ok(()) => Err(error),
            Err(rollback_error) => Err(rollback_error),
        },
    }
}

fn index_provider_inner(
    storage: &mut Storage,
    scan: ProviderScan<'_>,
    discovery: Discovery,
    summary: &mut IndexSummary,
    progress: &mut IndexProgress,
) -> Result<(), AppError> {
    index_provider_pipeline(
        storage,
        scan,
        discovery,
        summary,
        progress,
        &admission::Admission::new(),
    )
}

#[allow(
    clippy::too_many_lines,
    reason = "keep parser admission, producer/consumer and transaction boundaries in one scope"
)]
fn index_provider_pipeline(
    storage: &mut Storage,
    scan: ProviderScan<'_>,
    discovery: Discovery,
    summary: &mut IndexSummary,
    progress: &mut IndexProgress,
    admission: &admission::Admission,
) -> Result<(), AppError> {
    let ProviderScan {
        provider,
        roots,
        cutoff_modified_ns,
        parse,
        authoritative,
    } = scan;
    let observed_layout = discovery::source_layout(&discovery.sources);
    let (mut pending, observed_paths, mut complete) = prepare_sources(
        storage,
        provider,
        cutoff_modified_ns,
        authoritative,
        discovery,
        summary,
    )?;
    pending.sort_unstable_by(|left, right| {
        right
            .stamp
            .size_bytes
            .cmp(&left.stamp.size_bytes)
            .then_with(|| left.history.identity.path.cmp(&right.history.identity.path))
            .then_with(|| {
                left.history
                    .identity
                    .session
                    .cmp(&right.history.identity.session)
            })
    });
    progress.phase_started = Instant::now();
    progress.estimate = crate::progress::Estimate::starting_at(summary.processed_bytes);
    progress.parser_admission = admission.snapshot();
    progress.emit(summary, crate::progress::Phase::Discovered, true);

    let worker_count = parser_worker_count(pending.len());
    let next_source = AtomicUsize::new(0);
    let (sender, receiver) = sync_channel(1);
    let mut first_error = None;
    let mut batch_files = 0_u64;
    let mut batch_bytes = 0_u64;
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let pending = &pending;
            let next_source = &next_source;
            let context = crate::observability::ThreadContext::capture();
            scope.spawn(move || {
                context.run(|| {
                    loop {
                        let index = next_source.fetch_add(1, Ordering::Relaxed);
                        let Some(source) = pending.get(index) else {
                            break;
                        };
                        let reservation = admission.reserve(source.estimated_bytes);
                        let mut stamp = source.stamp.clone();
                        let (parsed, reservation) = match reservation {
                            Ok(reservation) => (
                                parse_source_with_retries(&source.history, &mut stamp, parse),
                                Some(reservation),
                            ),
                            Err(error) => (Err(error), None),
                        };
                        let parsed = parsed.map_err(|error| {
                            if authoritative {
                                configured_source_error(&source.history.identity.path, &error)
                            } else {
                                error
                            }
                        });
                        let source = SourceFile {
                            history: source.history.clone(),
                            stamp,
                            estimated_bytes: source.estimated_bytes,
                        };
                        if tracing::info_span!("queue_parsed_source").in_scope(|| {
                            sender
                                .send(ParsedSource {
                                    source,
                                    parsed,
                                    _reservation: reservation,
                                })
                                .is_err()
                        }) {
                            break;
                        }
                    }
                });
            });
        }
        drop(sender);

        loop {
            let result = match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(result) => result,
                Err(RecvTimeoutError::Timeout) => {
                    progress.parser_admission = admission.snapshot();
                    progress.emit(summary, crate::progress::Phase::Indexing, true);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let source_bytes = result.source.estimated_bytes;
            summary.processed_files += 1;
            summary.processed_bytes = summary.processed_bytes.saturating_add(source_bytes);
            let applied = apply_parsed_source(storage, provider, result, summary);
            progress.parser_admission = admission.snapshot();
            match applied {
                Ok(source_complete) => complete &= source_complete,
                Err(error) => {
                    record_first_error(&mut complete, &mut first_error, error);
                    continue;
                }
            }
            batch_files = batch_files.saturating_add(1);
            batch_bytes = batch_bytes.saturating_add(source_bytes);
            if !authoritative
                && (batch_files >= CHECKPOINT_FILES || batch_bytes >= CHECKPOINT_BYTES)
                && let Err(error) = checkpoint_provider_batch(
                    storage,
                    summary,
                    progress,
                    &mut batch_files,
                    &mut batch_bytes,
                )
            {
                record_first_error(&mut complete, &mut first_error, error);
            }
            progress.emit(summary, crate::progress::Phase::Indexing, false);
        }
    });
    progress.parser_admission = admission.snapshot();
    if authoritative {
        verify_authoritative_discovery(provider, roots, &observed_layout)?;
    }
    summary.purged_conversations += reconcile_complete_sources(
        storage,
        complete,
        provider,
        &observed_paths,
        roots,
        cutoff_modified_ns,
    )?;
    first_error.map_or(Ok(()), Err)
}

fn reconcile_complete_sources(
    storage: &mut Storage,
    complete: bool,
    provider: Provider,
    observed_keys: &BTreeSet<String>,
    roots: &[PathBuf],
    cutoff_modified_ns: Option<i64>,
) -> Result<u64, AppError> {
    if !complete {
        return Ok(0);
    }
    storage.purge_missing_sources(provider, observed_keys, roots, cutoff_modified_ns)
}

fn record_first_error(complete: &mut bool, first_error: &mut Option<AppError>, error: AppError) {
    *complete = false;
    if first_error.is_none() {
        *first_error = Some(error);
    }
}

#[tracing::instrument(name = "checkpoint_ingestion", skip_all, fields(files = *batch_files, bytes = *batch_bytes))]
fn checkpoint_provider_batch(
    storage: &mut Storage,
    summary: &mut IndexSummary,
    progress: &mut IndexProgress,
    batch_files: &mut u64,
    batch_bytes: &mut u64,
) -> Result<(), AppError> {
    storage.checkpoint_writer()?;
    summary.committed_batches += 1;
    *batch_files = 0;
    *batch_bytes = 0;
    progress.emit(summary, crate::progress::Phase::Checkpoint, true);
    Ok(())
}

#[tracing::instrument(name = "prepare_sources", skip_all, fields(provider = provider.as_str(), pending_sources = tracing::field::Empty))]
fn prepare_sources(
    storage: &Storage,
    provider: Provider,
    cutoff_modified_ns: Option<i64>,
    authoritative: bool,
    discovery: Discovery,
    summary: &mut IndexSummary,
) -> Result<(Vec<SourceFile>, BTreeSet<String>, bool), AppError> {
    let complete = discovery.complete;
    let mut pending = Vec::new();
    let mut observed_paths = BTreeSet::new();
    for source in discovery.sources {
        let identity = source.identity.clone();
        let path = &identity.path;
        observed_paths.insert(identity.key());
        let stamp = source.stamp().map_err(|error| {
            if authoritative {
                configured_source_error(path, &error)
            } else {
                error
            }
        })?;
        if cutoff_modified_ns
            .is_some_and(|cutoff| stamp.activity_ns.unwrap_or(stamp.modified_ns) < cutoff)
        {
            continue;
        }
        summary.scanned_files += 1;
        let bytes = source
            .estimated_bytes
            .unwrap_or_else(|| u64::try_from(stamp.size_bytes).unwrap_or_default());
        summary.discovered_bytes = summary.discovered_bytes.saturating_add(bytes);
        if storage.source_checkpoint_matches(provider, &identity, &stamp)? {
            summary.unchanged_sources += 1;
            summary.checkpoint_skipped_sources += 1;
            summary.processed_files += 1;
            summary.processed_bytes = summary.processed_bytes.saturating_add(bytes);
        } else {
            pending.push(SourceFile {
                history: source,
                stamp,
                estimated_bytes: bytes,
            });
        }
    }
    tracing::Span::current().record("pending_sources", pending.len());
    Ok((pending, observed_paths, complete))
}

fn configured_source_error(path: &Path, error: &AppError) -> AppError {
    if error.error.kind == crate::ErrorKind::SourceChanged {
        let mut changed = AppError::source_changed();
        changed.error.message = format!("{}: {}", path.display(), error.error.message);
        return changed;
    }
    AppError::configuration(format!(
        "configured provider source could not be indexed: {}: {}",
        path.display(),
        error.error.message
    ))
}

#[tracing::instrument(name = "parse_source", skip_all, fields(attempts = tracing::field::Empty))]
fn parse_source_with_retries(
    source: &HistorySource,
    stamp: &mut SourceStamp,
    parse: fn(&HistorySource) -> Result<ParsedFile, AppError>,
) -> Result<ParsedFile, AppError> {
    // Discovery can precede a source's turn in the bounded parser queue by
    // minutes. Capture a fresh stamp for each attempt, then require the whole
    // parse to be stable. Never retry malformed input or other parser errors.
    for attempt in 0..3 {
        tracing::Span::current().record("attempts", attempt + 1);
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(50));
        }
        *stamp = match source.stamp() {
            Ok(stamp) => stamp,
            Err(error) if error.error.kind == crate::ErrorKind::SourceChanged => continue,
            Err(error) => return Err(error),
        };
        match parse_stable_source(source, stamp, parse) {
            Err(error) if error.error.kind == crate::ErrorKind::SourceChanged => {}
            result => return result,
        }
    }
    Err(AppError::source_changed())
}

fn parse_stable_source(
    source: &HistorySource,
    stamp: &SourceStamp,
    parse: fn(&HistorySource) -> Result<ParsedFile, AppError>,
) -> Result<ParsedFile, AppError> {
    stamp.verify(&source.identity)?;
    let parsed = parse(source);
    stamp.verify(&source.identity)?;
    parsed
}

#[tracing::instrument(name = "apply_source", skip_all, fields(provider = provider.as_str()))]
fn apply_parsed_source(
    storage: &mut Storage,
    provider: Provider,
    mut result: ParsedSource<'_>,
    summary: &mut IndexSummary,
) -> Result<bool, AppError> {
    let mut parsed = result.parsed?;
    // A producer can wait in the bounded queue while another source is applied.
    // Discard stale normalized content before re-reading, keeping the same
    // bounded-memory behavior as the parser workers.
    if let Err(error) = result.source.stamp.verify(&result.source.history.identity) {
        if error.error.kind != crate::ErrorKind::SourceChanged {
            return Err(error);
        }
        drop(parsed);
        parsed = parse_source_with_retries(
            &result.source.history,
            &mut result.source.stamp,
            provider.parser(),
        )?;
    }
    summary.malformed_records += parsed.malformed_records;
    if let Some(conversation) = parsed.conversation {
        apply_conversation(storage, &conversation, summary)?;
    } else if parsed.malformed_records == 0
        && storage.remove_source(provider, &result.source.history.identity)?
    {
        summary.purged_conversations += 1;
    }
    if parsed.malformed_records == 0 {
        storage.record_source_checkpoint(
            provider,
            &result.source.history.identity,
            &result.source.stamp,
        )?;
    }
    Ok(parsed.malformed_records == 0)
}

impl IndexProgress {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last_event: now,
            phase_started: now,
            estimate: crate::progress::Estimate::new(),
            parser_admission: None,
        }
    }

    fn emit(&mut self, summary: &IndexSummary, phase: crate::progress::Phase, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_event) < Duration::from_secs(1) {
            return;
        }
        self.last_event = now;
        let elapsed_milliseconds =
            u64::try_from(now.duration_since(self.started).as_millis().max(1)).unwrap_or(u64::MAX);
        let eta = self.estimate.observe(
            summary.processed_bytes,
            summary.discovered_bytes,
            crate::progress::milliseconds(self.phase_started),
        );
        let event = crate::progress::Event {
            parser_admission: self.parser_admission.clone(),
            messages_per_second: Some(
                summary
                    .changed_messages
                    .saturating_mul(1_000)
                    .checked_div(elapsed_milliseconds)
                    .unwrap_or_default(),
            ),
            bytes_per_second: Some(
                summary
                    .processed_bytes
                    .saturating_mul(1_000)
                    .checked_div(elapsed_milliseconds)
                    .unwrap_or_default(),
            ),
            ..crate::progress::Event::files(
                phase,
                summary,
                elapsed_milliseconds,
                eta,
                crate::progress::EtaScope::ProviderIngestion,
            )
        };
        crate::progress::emit(&event);
    }
}

fn parser_worker_count(source_count: usize) -> usize {
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    source_count.min(available.saturating_sub(1).max(1)).min(8)
}

fn apply_conversation(
    storage: &mut Storage,
    conversation: &Conversation,
    summary: &mut IndexSummary,
) -> Result<(), AppError> {
    let change = storage.replace_conversation(conversation)?;
    summary.changed_messages += u64::try_from(change.changed_message_ids.len())
        .map_err(|_| AppError::internal("too many changed messages"))?;
    summary.removed_messages += change.removed_messages;
    summary.unchanged_sources += u64::from(change.unchanged);
    summary.tombstoned_sources += u64::from(change.tombstoned);
    Ok(())
}
