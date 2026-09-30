//! Routed Claude publication derives logical threads from the same source snapshots as SQLite.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{
    Checkpoint, DirectOwner, DirectScan, EmbeddingPool, Event, IndexResult, NodeState,
    replace_conversation, write_event,
};
use crate::AppError;
use crate::provider::Provider;
use crate::semantic;
use crate::storage::Storage;

const FINGERPRINT_VERSION: &str = "claude-provenance-v1:";

#[cfg(test)]
mod tests;

struct Plan {
    selected: BTreeSet<String>,
    dirty: BTreeSet<String>,
    checkpoints: Vec<Checkpoint>,
    observed: Vec<String>,
    skipped: u64,
}

fn plan(
    database: &Path,
    revision: i64,
    scan: &DirectScan,
    previous: &[Checkpoint],
    full: bool,
) -> Result<Plan, AppError> {
    let previous = previous
        .iter()
        .filter(|source| source.provider == Provider::ClaudeCode)
        .map(|source| (source.path_hash.clone(), source))
        .collect::<BTreeMap<_, _>>();
    if scan.cutoff.is_some()
        && previous
            .values()
            .any(|source| !source.fingerprint.starts_with(FINGERPRINT_VERSION))
    {
        return Err(AppError::shared(
            "routed Claude source identity upgrade requires an explicit `mash index --all-history` before publication",
        ));
    }
    let mut current = BTreeMap::new();
    let mut observed = Vec::new();
    for source in &scan.sources {
        let stamp = source.stamp()?;
        let hash = source.identity.publication_hash();
        observed.push(hash.clone());
        current.insert(hash, (&source.identity, stamp));
    }
    let mut plan = Plan {
        selected: BTreeSet::new(),
        dirty: BTreeSet::new(),
        checkpoints: Vec::new(),
        observed,
        skipped: 0,
    };
    let storage = Storage::open_existing(database)?;
    storage.with_publication_snapshot(|snapshot| {
        if snapshot.revision != revision { return Err(AppError::source_changed()); }
        let references = snapshot.claude_sources()?.into_iter().map(|source| (source.source.publication_hash(), source)).collect::<BTreeMap<_, _>>();
        for (hash, source) in &references {
            let admitted = current.get(hash).is_some_and(|(_, stamp)| scan.cutoff.is_none_or(|cutoff| stamp.modified_ns >= cutoff));
            let retained = previous.get(hash).is_some_and(|checkpoint| current.contains_key(hash) || scan.cutoff.is_some_and(|cutoff| checkpoint.modified_ns < cutoff));
            if admitted || retained { plan.selected.insert(source.source.key()); }
        }
        for (hash, (identity, stamp)) in &current {
            if scan.cutoff.is_some_and(|cutoff| stamp.modified_ns < cutoff) { continue; }
            if !storage.source_checkpoint_matches(Provider::ClaudeCode, identity, stamp)? {
                return Err(AppError::source_changed());
            }
            let conversation_id = references.get(hash).map(|source| source.conversation_id.clone());
            let fingerprint = format!("{FINGERPRINT_VERSION}{}", stamp.fingerprint);
            let old = previous.get(hash);
            let unchanged = !full && old.is_some_and(|checkpoint| checkpoint.ready
                && checkpoint.size_bytes == stamp.size_bytes && checkpoint.modified_ns == stamp.modified_ns
                && checkpoint.fingerprint == fingerprint && checkpoint.conversation_id == conversation_id);
            if unchanged { plan.skipped += 1; } else {
                plan.dirty.extend(conversation_id.clone());
                plan.dirty.extend(old.and_then(|checkpoint| checkpoint.conversation_id.clone()));
            }
            plan.checkpoints.push(Checkpoint { provider: Provider::ClaudeCode, path_hash: hash.clone(),
                size_bytes: stamp.size_bytes, modified_ns: stamp.modified_ns, fingerprint,
                conversation_id, ready: true });
        }
        if scan.complete {
            for (hash, checkpoint) in &previous {
                if !current.contains_key(hash) && scan.cutoff.is_none_or(|cutoff| checkpoint.modified_ns >= cutoff) {
                    plan.dirty.extend(checkpoint.conversation_id.clone());
                }
            }
        }
        // An old protected contribution absent from SQLite cannot be safely
        // reconstructed from a remote checkpoint. Require explicit replay.
        for (hash, checkpoint) in &previous {
            if checkpoint.conversation_id.as_ref().is_some_and(|id| plan.dirty.contains(id))
                && !references.contains_key(hash)
                && scan.cutoff.is_some_and(|cutoff| checkpoint.modified_ns < cutoff)
            {
                return Err(AppError::shared("retained Claude contributions require an explicit `mash index --all-history` before publication"));
            }
        }
        Ok(())
    })?;
    Ok(plan)
}

#[allow(
    clippy::too_many_arguments,
    reason = "same bounded publication and embedding pipeline as other native sources"
)]
#[tracing::instrument(name = "publish_claude_sources", skip_all)]
pub(super) async fn ingest(
    file: &mut std::fs::File,
    state: &NodeState,
    owner: &DirectOwner,
    database: &Path,
    scan: &DirectScan,
    full: bool,
    models: &Path,
    embeddings: Option<&semantic::remote::Configuration>,
    pool: &mut Option<EmbeddingPool>,
    result: &mut IndexResult,
) -> Result<(), AppError> {
    let db = database.to_path_buf();
    let revision = owner.revision;
    let previous = state.sources.clone();
    let input = scan.clone();
    let plan = crate::blocking(move || plan(&db, revision, &input, &previous, full)).await?;
    result.summary.scanned_files += plan.checkpoints.len() as u64;
    result.summary.processed_files += plan.checkpoints.len() as u64;
    result.summary.unchanged_sources += plan.skipped;
    result.summary.checkpoint_skipped_sources += plan.skipped;
    for id in &plan.dirty {
        let db = database.to_path_buf();
        let keys = plan.selected.clone();
        let thread = id.clone();
        let conversation = crate::blocking(move || {
            let storage = Storage::open_existing(&db)?;
            let mut conversation = None;
            storage.with_publication_snapshot(|snapshot| {
                if snapshot.revision != revision {
                    return Err(AppError::source_changed());
                }
                conversation = snapshot.claude_thread(&thread, &keys)?;
                Ok(())
            })?;
            Ok(conversation)
        })
        .await?;
        if let Some(conversation) = conversation {
            let forgotten = state
                .tombstones
                .iter()
                .any(|t| t.provider == Provider::ClaudeCode && t.conversation_id == *id)
                || owner.forgets.contains(id);
            if forgotten {
                result.summary.tombstoned_sources += 1;
            } else {
                replace_conversation(file, &conversation, models, embeddings, pool, result).await?;
            }
        } else {
            write_event(file, &Event::Remove { id: id.clone() })?;
        }
    }
    for checkpoint in plan.checkpoints {
        write_event(file, &Event::Source(checkpoint))?;
    }
    for hashes in plan.observed.chunks(128) {
        write_event(
            file,
            &Event::Seen {
                provider: Provider::ClaudeCode,
                hashes: hashes.to_vec(),
            },
        )?;
    }
    if scan.complete {
        write_event(
            file,
            &Event::Reconcile {
                provider: Provider::ClaudeCode,
                cutoff: scan.cutoff,
            },
        )?;
    }
    let verification = scan.clone();
    crate::blocking(move || verification.verify()).await
}
