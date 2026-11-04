//! Apply one validated node upload to shared silver inside a single transaction.
use super::{Head, Identity, database, encode, head, now, provider, put_head, sql, token};
use crate::shared::protocol::{
    self, Batch, ConversationRecord, Event, MessageRecord, Start, Tombstone,
};
use crate::shared::reuse::Fingerprint;
use crate::shared::validation::{conflict, valid_id};
use crate::{AppError, provider::Provider, views::config::Build};
use rusqlite::Transaction;
use std::collections::BTreeSet;
use std::io::BufRead;

#[cfg(test)]
mod tests;

/// The caller holds the node lock through this operation and subsequent views
/// synchronization. Duplicate validation rolls back the outer transaction so
/// SQLite need not retain a snapshot-sized nested-savepoint subjournal.
pub(in crate::shared) fn write(
    store: &super::SilverStore,
    identity: &Identity,
    file: &tempfile::NamedTempFile,
    build: &Build,
    progress: &crate::shared::activity::Progress,
    receipt: Option<&super::SyncReceipt>,
    rebase: bool,
) -> Result<Applied, AppError> {
    write_inner(
        store,
        identity,
        file,
        build,
        progress,
        receipt,
        rebase,
        || Ok(()),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "one transaction needs identity, file, build, progress, receipt and rebase"
)]
fn write_inner(
    store: &super::SilverStore,
    identity: &Identity,
    file: &tempfile::NamedTempFile,
    build: &Build,
    progress: &crate::shared::activity::Progress,
    receipt: Option<&super::SyncReceipt>,
    rebase: bool,
    after_rollback: impl FnOnce() -> Result<(), AppError>,
) -> Result<Applied, AppError> {
    let mut connection = store.connect(identity)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(database)?;
    let applied = apply(
        &transaction,
        identity,
        file,
        build,
        progress,
        receipt,
        rebase,
    )?;
    progress.phase(
        crate::shared::activity::Phase::CommittingSilver,
        crate::shared::activity::Unit::Records,
    );
    if applied.duplicate {
        transaction.rollback().map_err(database)?;
        // A crash here leaves the previously committed history and head intact.
        // The node lock prevents another writer between rollback and this small
        // last-synchronized metadata update; no Qdrant work has started yet.
        after_rollback()?;
        let metadata = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database)?;
        put_head(&metadata, &applied.head)?;
        metadata.commit().map_err(database)?;
    } else {
        transaction.commit().map_err(database)?;
    }
    Ok(applied)
}

#[derive(Debug)]
pub(in crate::shared) struct Applied {
    pub(in crate::shared) head: Head,
    pub(in crate::shared) revision: i64,
    /// Previously accepted snapshot with matching canonical history.
    pub(in crate::shared) duplicate: bool,
    pub(in crate::shared) changed: bool,
    pub(in crate::shared) removed: u64,
    pub(in crate::shared) purged: u64,
    /// Chats whose messages and vectors this upload supplies.
    pub(in crate::shared) supplied: BTreeSet<String>,
    /// Chats removed from silver; their views documents are obsolete.
    pub(in crate::shared) deleted: BTreeSet<String>,
}

struct Upload<'a> {
    tx: &'a Transaction<'a>,
    identity: &'a Identity,
    build: &'a Build,
    applied: Applied,
}

fn first_line(
    file: &tempfile::NamedTempFile,
) -> Result<(Start, impl Iterator<Item = std::io::Result<String>>), AppError> {
    let mut lines = std::io::BufReader::new(
        file.reopen()
            .map_err(|_| AppError::internal("open incoming index"))?,
    )
    .lines();
    let first = lines
        .next()
        .ok_or_else(|| AppError::usage("empty index stream"))?
        .map_err(|_| AppError::usage("invalid index stream"))?;
    let Event::Start(input) =
        serde_json::from_str(&first).map_err(|_| AppError::usage("invalid index start"))?
    else {
        return Err(AppError::usage("index stream must start with metadata"));
    };
    Ok((input, lines))
}

/// Validate revision ordering; returns the accepted revision and whether this
/// is a snapshot of the already-accepted revision.
fn start(previous: Option<&Head>, input: &Start) -> Result<(i64, bool), AppError> {
    input.validate_source_silver()?;
    valid_id(&input.instance)?;
    if input.revision.is_some_and(|r| r < 1)
        || input.base_revision.is_some_and(|r| r < 0)
        || input.forgets.len() > 10000
        || (input.snapshot && (input.base_revision.is_some() || !input.forgets.is_empty()))
    {
        return Err(AppError::usage("invalid index metadata"));
    }
    if previous.is_some_and(|h| h.instance != input.instance) {
        return Err(conflict(
            "node_id belongs to another database or indexing mode",
        ));
    }
    let accepted = previous.map_or(0, |h| h.revision);
    if input.snapshot {
        let revision = input
            .revision
            .ok_or_else(|| AppError::usage("snapshot requires revision"))?;
        if revision < accepted {
            return Err(conflict("shared index has a newer snapshot"));
        }
        return Ok((revision, revision == accepted));
    }
    if input.base_revision != Some(accepted) || input.revision.is_some_and(|r| r <= accepted) {
        return Err(conflict(
            "shared index changed while preparing upload; retry indexing",
        ));
    }
    let revision = input.revision.unwrap_or(
        accepted
            .checked_add(1)
            .ok_or_else(|| conflict("node revision exhausted"))?,
    );
    Ok((revision, false))
}

impl Upload<'_> {
    fn provider(&self, id: &str) -> Result<Option<Provider>, AppError> {
        sql::ConversationProvider::builder()
            .id(id)
            .build()
            .query_opt(self.tx)
            .map_err(database)?
            .map(|row| provider(&row.provider))
            .transpose()
    }

    fn tombstoned(&self, provider: Provider, id: &str) -> Result<bool, AppError> {
        Ok(sql::Tombstoned::builder()
            .provider(provider.as_str())
            .conversation_id(id)
            .build()
            .query_opt(self.tx)
            .map_err(database)?
            .is_some())
    }

    /// Remove one thread; returns removed messages and threads.
    fn remove(&mut self, id: &str) -> Result<(u64, u64), AppError> {
        let messages = sql::DeleteThreadMessages::builder()
            .conversation_id(id)
            .build()
            .execute(self.tx)
            .map_err(database)?;
        let threads = sql::DeleteConversation::builder()
            .id(id)
            .build()
            .execute(self.tx)
            .map_err(database)?;
        if threads != 0 {
            self.applied.deleted.insert(id.to_owned());
            self.applied.supplied.remove(id);
        }
        Ok((messages as u64, threads as u64))
    }

    fn tombstone(&mut self, record: &Tombstone) -> Result<(), AppError> {
        let forgotten_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AppError::internal("system clock precedes Unix epoch"))?
            .as_millis();
        sql::UpsertTombstone::builder()
            .provider(record.provider.as_str())
            .conversation_id(&record.conversation_id)
            .forgotten_at(i64::try_from(forgotten_at).unwrap_or(i64::MAX))
            .build()
            .execute(self.tx)
            .map_err(database)?;
        Ok(())
    }

    fn forget(&mut self, id: &str) -> Result<bool, AppError> {
        valid_id(id)?;
        let providers = self
            .provider(id)?
            .map_or_else(|| Provider::ALL.to_vec(), |p| vec![p]);
        let (_, removed) = self.remove(id)?;
        for provider in providers {
            self.tombstone(&Tombstone {
                provider,
                conversation_id: id.to_owned(),
            })?;
        }
        Ok(removed > 0)
    }

    fn conversation(&mut self, record: ConversationRecord, snapshot: bool) -> Result<(), AppError> {
        record.validate_enrichment()?;
        valid_id(&record.id)?;
        if !snapshot {
            self.remove(&record.id)?;
        } else if self.provider(&record.id)?.is_some() {
            return Err(AppError::usage("duplicate conversation"));
        }
        if self.tombstoned(record.provider, &record.id)? {
            return Ok(());
        }
        sql::InsertConversation::builder()
            .provider(record.provider.as_str())
            .id(&record.id)
            .kind(&format!("{}:chat", record.provider.as_str()))
            .title(record.title.as_deref())
            .created_at(record.created_at)
            .updated_at(record.updated_at)
            .project(record.project.as_deref())
            .repository(record.repository.as_deref())
            .build()
            .execute(self.tx)
            .map_err(database)?;
        sql::InsertNativeChat::builder()
            .id(&record.id)
            .build()
            .execute(self.tx)
            .map_err(database)?;
        if let Some(evidence) = &record.enrichment {
            sql::InsertEnrichment::builder()
                .conversation_id(&record.id)
                .evidence(&encode(evidence)?)
                .build()
                .execute(self.tx)
                .map_err(database)?;
        }
        self.applied.deleted.remove(&record.id);
        self.applied.supplied.insert(record.id);
        self.applied.changed = true;
        Ok(())
    }

    /// Reject records views could not use under this recipe and model space.
    fn check_message(&self, record: &MessageRecord) -> Result<(), AppError> {
        valid_id(&record.id)?;
        valid_id(&record.conversation_id)?;
        crate::attachments::validate_metadata(&record.attachments)?;
        record.tools.validate()?;
        if record.ordinal < 0
            || (!self.build.includes_dense(&record.role, &record.projection)
                && (record.vector.is_some() || record.generation.is_some()))
            || !record.context.valid(record.ordinal)
            || record
                .vector
                .as_ref()
                .is_some_and(|v| !protocol::valid_vector(v, self.identity.dimensions))
            || (record.projection.is_empty()
                && (record.vector.is_some() || record.generation.is_some()))
        {
            return Err(AppError::usage("invalid message embedding or ordinal"));
        }
        Ok(())
    }

    fn message(&mut self, mut record: MessageRecord) -> Result<(), AppError> {
        self.check_message(&record)?;
        if self.provider(&record.conversation_id)?.is_none() {
            return Ok(());
        }
        // Vectors are views; silver keeps the canonical record only.
        record.vector = None;
        record.generation = None;
        let inserted = sql::InsertMessage::builder()
            .attachments(&encode(&record.attachments)?)
            .id(&record.id)
            .conversation_id(&record.conversation_id)
            .ordinal(Some(record.ordinal))
            .role(Some(record.role.as_str()))
            .content(&record.content)
            .search_projection(Some(&record.projection))
            .created_at(record.created_at)
            .context_parent(record.context.parent())
            .metadata(record.tools.encode()?.as_deref())
            .build()
            .execute(self.tx);
        match inserted {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(error, Some(message)))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(AppError::usage(if message.contains("ordinal") {
                    "duplicate message ordinal"
                } else {
                    "duplicate message"
                }));
            }
            Err(error) => return Err(database(error)),
        }
        self.applied.changed = true;
        Ok(())
    }

    /// Recompute dependency fingerprints for threads written by this upload.
    fn fingerprint(&self, id: &str) -> Result<String, AppError> {
        let conversation = super::conversation(self.tx, id)?
            .ok_or_else(|| AppError::internal("fingerprinted thread is missing"))?;
        let mut fingerprint = Fingerprint::new(&conversation)?;
        for message in super::thread_messages(self.tx, id)? {
            fingerprint.push(&message)?;
        }
        Ok(fingerprint.finish())
    }

    fn missing_parents(&self) -> Result<bool, AppError> {
        for id in &self.applied.supplied {
            let missing = sql::MissingParent::builder()
                .conversation_id(id)
                .build()
                .query_opt(self.tx)
                .map_err(database)?;
            if missing.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Canonical identity over thread fingerprints and deletion intent.
    fn canonical(&self) -> Result<String, AppError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"mash-shared-silver-v2\0");
        for (id, fingerprint) in super::fingerprints(self.tx)? {
            for part in [id.as_str(), fingerprint.as_str()] {
                hasher.update(&(part.len() as u64).to_le_bytes());
                hasher.update(part.as_bytes());
            }
        }
        hasher.update(b"\0tombstones\0");
        for tombstone in super::tombstones(self.tx)? {
            let encoded = encode(&tombstone)?;
            hasher.update(&(encoded.len() as u64).to_le_bytes());
            hasher.update(encoded.as_bytes());
        }
        Ok(hasher.finalize().to_hex().to_string())
    }
}

/// Validate and stage the complete upload; the caller owns commit or rollback.
#[allow(
    clippy::too_many_lines,
    reason = "one protocol boundary: validate every event before the transaction commits"
)]
fn apply(
    tx: &Transaction<'_>,
    identity: &Identity,
    file: &tempfile::NamedTempFile,
    build: &Build,
    progress: &crate::shared::activity::Progress,
    receipt: Option<&super::SyncReceipt>,
    rebase: bool,
) -> Result<Applied, AppError> {
    let previous = head(tx)?;
    let (mut input, lines) = first_line(file)?;
    // A baseline batch merges by conversation, so its ordering token only has
    // to follow whatever committed before it. The head captured while preparing
    // is stale when another lane committed first; rebase here, inside the write
    // transaction, so concurrent lanes still get strictly ordered revisions.
    if rebase && !input.snapshot {
        let accepted = previous.as_ref().map_or(0, |head| head.revision);
        input.base_revision = Some(accepted);
        input.revision = Some(accepted.saturating_add(1));
    }
    // A resent revision changes no silver; views synchronization still runs
    // when views are stale, including after a group recipe change.
    let (revision, duplicate) = start(previous.as_ref(), &input)?;
    if input
        .views_build
        .as_ref()
        .is_some_and(|requested| requested != build)
    {
        return Err(conflict("shared views recipe changed; retry indexing"));
    }
    let mut upload = Upload {
        tx,
        identity,
        build,
        applied: Applied {
            head: previous.clone().unwrap_or(Head {
                instance: input.instance.clone(),
                revision: 0,
                version: String::new(),
                silver_revision: None,
                source_silver: None,
                last_synced_at: None,
                last_updated_at: None,
                operation_id: None,
                manifest_digest: None,
                receipt_digest: None,
            }),
            revision,
            duplicate,
            changed: false,
            removed: 0,
            purged: 0,
            supplied: BTreeSet::new(),
            deleted: BTreeSet::new(),
        },
    };
    upload
        .applied
        .head
        .source_silver
        .clone_from(&input.source_silver);
    for id in &input.forgets {
        upload.applied.changed |= upload.forget(id)?;
    }
    if input.snapshot {
        let existing = super::fingerprints(tx)?;
        sql::DeleteNodeMessages.execute(tx).map_err(database)?;
        sql::DeleteNodeConversations.execute(tx).map_err(database)?;
        upload.applied.deleted.extend(existing.into_keys());
        upload.applied.changed = true;
    }
    let mut finished = false;
    for line in lines {
        if finished {
            return Err(AppError::usage("data after index finish"));
        }
        let event: Event =
            serde_json::from_str(&line.map_err(|_| AppError::usage("invalid index stream"))?)
                .map_err(|_| AppError::usage("invalid index event"))?;
        if matches!(event, Event::Finish) {
            finished = true;
            continue;
        }
        if matches!(event, Event::Start(_)) {
            return Err(AppError::usage("duplicate index start"));
        }
        match event {
            Event::Batch(batch) => {
                if batch.len() > 128 {
                    return Err(AppError::usage("index batch exceeds 128 records"));
                }
                match batch {
                    Batch::Tombstones { records } => {
                        for record in records {
                            valid_id(&record.conversation_id)?;
                            if upload.provider(&record.conversation_id)? == Some(record.provider) {
                                upload.remove(&record.conversation_id)?;
                            }
                            upload.tombstone(&record)?;
                            upload.applied.changed = true;
                            progress.advance(1);
                        }
                    }
                    Batch::Conversations { records } => {
                        for record in records {
                            upload.conversation(record, input.snapshot)?;
                            progress.advance(1);
                        }
                    }
                    Batch::Messages { records } => {
                        for record in records {
                            upload.message(record)?;
                            progress.advance(1);
                        }
                    }
                }
            }
            Event::Remove { id } if !input.snapshot => {
                valid_id(&id)?;
                let (m, c) = upload.remove(&id)?;
                upload.applied.removed += m;
                upload.applied.purged += c;
                upload.applied.changed |= c > 0;
            }
            _ => {
                return Err(AppError::usage(
                    "invalid index event for this indexing mode",
                ));
            }
        }
    }
    if !finished {
        return Err(AppError::usage("incomplete index stream"));
    }
    if upload.missing_parents()? {
        return Err(AppError::usage("missing message context parent"));
    }
    let supplied = upload.applied.supplied.clone();
    progress.phase(
        crate::shared::activity::Phase::FinalizingSilver,
        crate::shared::activity::Unit::Chats,
    );
    for id in &supplied {
        crate::storage::turns::derive(tx, id)?;
        let fingerprint = upload.fingerprint(id)?;
        sql::SetFingerprint::builder()
            .fingerprint(Some(&fingerprint))
            .id(id)
            .build()
            .execute(tx)
            .map_err(database)?;
        progress.advance(1);
    }
    let canonical = upload.canonical()?;
    let mut applied = upload.applied;
    if duplicate {
        let retained = previous.as_ref().and_then(|h| h.silver_revision.as_ref());
        if retained.is_none_or(|state| state.fingerprint != canonical) {
            return Err(conflict(
                "snapshot revision already committed with different silver",
            ));
        }
        applied.head = previous.ok_or_else(|| AppError::internal("duplicate without head"))?;
        applied.head.last_synced_at = Some(now());
        applied.changed = false;
        stamp_receipt(&mut applied.head, receipt);
        return Ok(applied);
    }
    let now = now();
    applied.head.silver_revision = Some(crate::shared::silver_revision::State::next(
        applied.head.silver_revision.as_ref(),
        canonical,
    )?);
    applied.head.revision = revision;
    applied.head.version = token()?;
    applied.head.last_synced_at = Some(now.clone());
    if applied.changed {
        applied.head.last_updated_at = Some(now);
    }
    stamp_receipt(&mut applied.head, receipt);
    put_head(tx, &applied.head)?;
    Ok(applied)
}

/// Record the transfer receipt on the head inside the silver transaction, so a
/// receipt exists if and only if the described history committed.
fn stamp_receipt(head: &mut Head, receipt: Option<&super::SyncReceipt>) {
    let Some(receipt) = receipt else {
        return;
    };
    head.receipt_digest = Some(super::receipt_digest(
        &receipt.manifest_digest,
        head.revision,
    ));
    head.manifest_digest = Some(receipt.manifest_digest.clone());
    head.operation_id = Some(receipt.operation_id.clone());
}
