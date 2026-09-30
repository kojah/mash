use crate::provider::Provider;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant, SystemTime};

mod aider;
mod discovery;
mod json;
mod parsers;
#[cfg(test)]
mod tests;

pub(crate) use discovery::{DirectScan, direct_scans, source_stamp};
use discovery::{
    Discovery, cutoff_modified_ns, discover_history_files, verify_authoritative_discovery,
};

use crate::AppError;
use crate::storage::Storage;

const MAX_TOOL_OUTPUT_CHARS: usize = 128 * 1024;
const CHECKPOINT_FILES: u64 = 32;
const CHECKPOINT_BYTES: u64 = 128 * 1024 * 1024;

impl Provider {
    fn parser(self) -> fn(&Path) -> Result<ParsedFile, AppError> {
        match self {
            Self::ClaudeCode => parsers::parse_claude,
            Self::Codex => parsers::parse_codex,
            Self::Aider => aider::parse,
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
    pub(crate) source_path: PathBuf,
    pub(crate) project: Option<String>,
    pub(crate) repository: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) created_at: Option<i64>,
    pub(crate) updated_at: Option<i64>,
    pub(crate) messages: Vec<NormalizedMessage>,
}

pub(crate) struct NormalizedMessage {
    pub(crate) id: String,
    pub(crate) ordinal: i64,
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
    parse: fn(&Path) -> Result<ParsedFile, AppError>,
    authoritative: bool,
}

struct SourceFile {
    path: PathBuf,
    source_path: String,
    size_bytes: i64,
    modified_ns: i64,
}

struct ParsedSource {
    source: SourceFile,
    parsed: Result<ParsedFile, AppError>,
}

struct IndexProgress {
    started: Instant,
    last_event: Instant,
    phase_started: Instant,
    estimate: crate::progress::Estimate,
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

#[allow(
    clippy::too_many_lines,
    reason = "keep the bounded parser producer/consumer transaction in one linear scope"
)]
fn index_provider_inner(
    storage: &mut Storage,
    scan: ProviderScan<'_>,
    discovery: Discovery,
    summary: &mut IndexSummary,
    progress: &mut IndexProgress,
) -> Result<(), AppError> {
    let ProviderScan {
        provider,
        roots,
        cutoff_modified_ns,
        parse,
        authoritative,
    } = scan;
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
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    progress.phase_started = Instant::now();
    progress.estimate = crate::progress::Estimate::starting_at(summary.processed_bytes);
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
            scope.spawn(move || {
                loop {
                    let index = next_source.fetch_add(1, Ordering::Relaxed);
                    let Some(source) = pending.get(index) else {
                        break;
                    };
                    let parsed = parse(&source.path).map_err(|error| {
                        if authoritative {
                            configured_source_error(&source.path, &error)
                        } else {
                            error
                        }
                    });
                    let source = SourceFile {
                        path: source.path.clone(),
                        source_path: source.source_path.clone(),
                        size_bytes: source.size_bytes,
                        modified_ns: source.modified_ns,
                    };
                    if sender.send(ParsedSource { source, parsed }).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        loop {
            let result = match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(result) => result,
                Err(RecvTimeoutError::Timeout) => {
                    progress.emit(summary, crate::progress::Phase::Indexing, true);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let source_bytes = u64::try_from(result.source.size_bytes).unwrap_or_default();
            summary.processed_files += 1;
            summary.processed_bytes = summary.processed_bytes.saturating_add(source_bytes);
            match apply_parsed_source(storage, provider, result, summary) {
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
    if authoritative {
        verify_authoritative_discovery(provider, roots, &observed_paths)?;
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
    observed_paths: &BTreeSet<String>,
    roots: &[PathBuf],
    cutoff_modified_ns: Option<i64>,
) -> Result<u64, AppError> {
    if !complete {
        return Ok(0);
    }
    storage.purge_missing_sources(provider, observed_paths, roots, cutoff_modified_ns)
}

fn record_first_error(complete: &mut bool, first_error: &mut Option<AppError>, error: AppError) {
    *complete = false;
    if first_error.is_none() {
        *first_error = Some(error);
    }
}

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
    for path in discovery.files {
        let source_path = path.to_string_lossy().into_owned();
        observed_paths.insert(source_path.clone());
        let (size_bytes, modified_ns) = source_stamp(&path).map_err(|error| {
            if authoritative {
                configured_source_error(&path, &error)
            } else {
                error
            }
        })?;
        if cutoff_modified_ns.is_some_and(|cutoff| modified_ns < cutoff) {
            continue;
        }
        summary.scanned_files += 1;
        let bytes = u64::try_from(size_bytes).unwrap_or_default();
        summary.discovered_bytes = summary.discovered_bytes.saturating_add(bytes);
        if storage.source_checkpoint_matches(provider, &source_path, size_bytes, modified_ns)? {
            summary.unchanged_sources += 1;
            summary.checkpoint_skipped_sources += 1;
            summary.processed_files += 1;
            summary.processed_bytes = summary.processed_bytes.saturating_add(bytes);
        } else {
            pending.push(SourceFile {
                path,
                source_path,
                size_bytes,
                modified_ns,
            });
        }
    }
    Ok((pending, observed_paths, complete))
}

fn configured_source_error(path: &Path, error: &AppError) -> AppError {
    AppError::configuration(format!(
        "configured provider source became inaccessible: {}: {}",
        path.display(),
        error.error.message
    ))
}

fn apply_parsed_source(
    storage: &mut Storage,
    provider: Provider,
    result: ParsedSource,
    summary: &mut IndexSummary,
) -> Result<bool, AppError> {
    let parsed = result.parsed?;
    summary.malformed_records += parsed.malformed_records;
    if let Some(conversation) = parsed.conversation {
        apply_conversation(storage, &conversation, summary)?;
    } else if parsed.malformed_records == 0
        && storage.remove_source(provider, &result.source.source_path)?
    {
        summary.purged_conversations += 1;
    }
    if parsed.malformed_records == 0 {
        storage.record_source_checkpoint(
            provider,
            &result.source.source_path,
            result.source.size_bytes,
            result.source.modified_ns,
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
