//! Apply one validated node upload to shared silver inside a single transaction.
use super::{
    Head, Identity, checkpoint, database, encode, head, now, provider, put_head, sql, token,
};
use crate::shared::protocol::{
    self, Batch, ConversationRecord, Event, MessageRecord, ScanReport, Start, Tombstone,
};
use crate::shared::reuse::Fingerprint;
use crate::shared::validation::{conflict, valid_id};
use crate::{AppError, gold::config::Build, provider::Provider};
use rusqlite::Transaction;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::BufRead;

/// Gold facts the publication protocol depends on, read from Qdrant beforehand.
#[derive(Default)]
pub(in crate::shared) struct GoldView {
    /// Recipe of current, complete gold for the retained silver revision.
    pub(in crate::shared) build: Option<Build>,
    /// Thread fingerprints whose complete gold may be reused by a snapshot.
    pub(in crate::shared) reusable: BTreeMap<String, String>,
}

#[derive(Debug)]
pub(in crate::shared) struct Applied {
    pub(in crate::shared) head: Head,
    pub(in crate::shared) revision: i64,
    /// Same source revision and recipe: silver unchanged, upload ignored.
    pub(in crate::shared) duplicate: bool,
    pub(in crate::shared) changed: bool,
    pub(in crate::shared) removed: u64,
    pub(in crate::shared) purged: u64,
    /// Threads whose messages and vectors this upload supplies.
    pub(in crate::shared) supplied: BTreeSet<String>,
    /// Unchanged snapshot threads whose existing gold is retained.
    pub(in crate::shared) reused: BTreeSet<String>,
    /// Threads removed from silver; their gold documents are obsolete.
    pub(in crate::shared) deleted: BTreeSet<String>,
}

struct Upload<'a> {
    tx: &'a Transaction<'a>,
    identity: &'a Identity,
    build: &'a Build,
    applied: Applied,
    vectors: HashSet<String>,
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

/// Validate pins and revision ordering; returns the accepted revision and
/// whether this is a snapshot of the already-accepted revision.
fn start(previous: Option<&Head>, expected: &str, input: &Start) -> Result<(i64, bool), AppError> {
    input.validate_source_silver()?;
    valid_id(&input.instance)?;
    if input.revision.is_some_and(|r| r < 1)
        || input.base_revision.is_some_and(|r| r < 0)
        || input
            .expected_version
            .as_ref()
            .is_some_and(|v| v.len() > 4096)
        || input.forgets.len() > 10000
        || (input.reuse_gold
            && (!input.snapshot || input.expected_version.is_none() || input.gold_build.is_none()))
        || (input.snapshot && (input.base_revision.is_some() || !input.forgets.is_empty()))
    {
        return Err(AppError::usage("invalid index metadata"));
    }
    if input
        .expected_version
        .as_ref()
        .is_some_and(|version| version != expected)
    {
        return Err(conflict(
            "shared publication changed while preparing upload; retry indexing",
        ));
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
            .id(&record.id)
            .provider(record.provider.as_str())
            .title(record.title.as_deref())
            .created_at(record.created_at)
            .updated_at(record.updated_at)
            .project(record.project.as_deref())
            .repository(record.repository.as_deref())
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

    fn message(&mut self, mut record: MessageRecord) -> Result<(), AppError> {
        valid_id(&record.id)?;
        valid_id(&record.conversation_id)?;
        crate::attachments::validate_metadata(&record.attachments)?;
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
        if self.provider(&record.conversation_id)?.is_none() {
            return Ok(());
        }
        if record.vector.is_some() || record.generation.is_some() {
            self.vectors.insert(record.conversation_id.clone());
        }
        // Vectors are gold; silver keeps the canonical record only.
        record.vector = None;
        record.generation = None;
        let inserted = sql::InsertMessage::builder()
            .attachments(&encode(&record.attachments)?)
            .id(&record.id)
            .conversation_id(&record.conversation_id)
            .ordinal(record.ordinal)
            .role(record.role.as_str())
            .content(&record.content)
            .search_projection(Some(&record.projection))
            .created_at(record.created_at)
            .context_parent(record.context.parent())
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

    fn source(&mut self, checkpoint: &protocol::Checkpoint) -> Result<(), AppError> {
        sql::UpsertSource::builder()
            .provider(checkpoint.provider.as_str())
            .source_key(&checkpoint.path_hash)
            .size_bytes(checkpoint.size_bytes)
            .modified_ns(checkpoint.modified_ns)
            .fingerprint(&checkpoint.fingerprint)
            .interpretation_version(i64::from(checkpoint.interpretation_version))
            .conversation_id(checkpoint.conversation_id.as_deref())
            .ready(i64::from(checkpoint.ready))
            .build()
            .execute(self.tx)
            .map_err(database)?;
        Ok(())
    }

    fn reconcile(
        &mut self,
        provider: Provider,
        cutoff: Option<i64>,
        observed: Option<&HashSet<String>>,
    ) -> Result<(), AppError> {
        let rows = sql::ProviderSources::builder()
            .provider(provider.as_str())
            .build()
            .query_many(self.tx)
            .map_err(database)?;
        let mut missing = Vec::new();
        for row in rows {
            let source: protocol::Checkpoint = checkpoint!(row)?;
            if observed.is_none_or(|seen| !seen.contains(&source.path_hash))
                && cutoff.is_none_or(|c| source.modified_ns >= c)
            {
                sql::DeleteSource::builder()
                    .provider(source.provider.as_str())
                    .source_key(&source.path_hash)
                    .build()
                    .execute(self.tx)
                    .map_err(database)?;
                missing.extend(source.conversation_id);
            }
        }
        for id in missing {
            let referenced = sql::SourceReference::builder()
                .conversation_id(Some(&id))
                .build()
                .query_opt(self.tx)
                .map_err(database)?
                .is_some();
            if !referenced {
                let (m, c) = self.remove(&id)?;
                self.applied.removed += m;
                self.applied.purged += c;
                self.applied.changed |= c > 0;
            }
        }
        Ok(())
    }

    fn affected(&self, event: &Event) -> Result<Vec<Provider>, AppError> {
        let thread = |id: &str| self.provider(id);
        Ok(match event {
            Event::Batch(Batch::Conversations { records }) => {
                let mut providers = Vec::new();
                for record in records {
                    providers.push(record.provider);
                    providers.extend(thread(&record.id)?);
                }
                providers
            }
            Event::Batch(Batch::Messages { records }) => {
                let mut providers = Vec::new();
                for record in records {
                    providers.extend(thread(&record.conversation_id)?);
                }
                providers
            }
            Event::Batch(Batch::Tombstones { records }) => {
                records.iter().map(|t| t.provider).collect()
            }
            Event::Source(source) => vec![source.provider],
            Event::Seen { provider, .. } | Event::Reconcile { provider, .. } => vec![*provider],
            Event::Remove { id } => thread(id)?.into_iter().collect(),
            Event::Start(_) | Event::Scan(_) | Event::ReuseGold { .. } | Event::Finish => {
                Vec::new()
            }
        })
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

/// Validate the complete upload and commit its silver changes, or nothing.
#[allow(
    clippy::too_many_lines,
    reason = "one protocol boundary: validate every event before the transaction commits"
)]
pub(in crate::shared) fn apply(
    tx: &Transaction<'_>,
    identity: &Identity,
    file: &tempfile::NamedTempFile,
    build: &Build,
    gold: &GoldView,
) -> Result<Applied, AppError> {
    let previous = head(tx)?;
    let expected = previous.as_ref().map_or("", |h| h.version.as_str());
    let (input, lines) = first_line(file)?;
    let (revision, same_revision) = start(previous.as_ref(), expected, &input)?;
    if input
        .gold_build
        .as_ref()
        .is_some_and(|requested| requested != build)
    {
        return Err(conflict("shared gold recipe changed; retry indexing"));
    }
    let requested = input.gold_build.clone().unwrap_or_default();
    // A same-revision snapshot under a new recipe refreshes gold, never silver.
    let gold_refresh = same_revision
        && input.expected_version.is_some()
        && input.gold_build.is_some()
        && gold.build.as_ref() != Some(&requested);
    let duplicate = same_revision && !gold_refresh;
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
                silver_coverage: Vec::new(),
                silver_scans: Vec::new(),
                last_synced_at: None,
                last_updated_at: None,
            }),
            revision,
            duplicate,
            changed: false,
            removed: 0,
            purged: 0,
            supplied: BTreeSet::new(),
            reused: BTreeSet::new(),
            deleted: BTreeSet::new(),
        },
        vectors: HashSet::new(),
    };
    let before = gold_refresh.then(|| upload.canonical()).transpose()?;
    let mut dirty = HashSet::new();
    if !duplicate {
        upload
            .applied
            .head
            .source_silver
            .clone_from(&input.source_silver);
        for id in &input.forgets {
            if let Some(provider) = upload.provider(id)? {
                dirty.insert(provider);
            }
            upload.applied.changed |= upload.forget(id)?;
        }
        if input.snapshot {
            let existing = super::fingerprints(tx)?;
            sql::DeleteNodeMessages.execute(tx).map_err(database)?;
            sql::DeleteNodeConversations.execute(tx).map_err(database)?;
            sql::DeleteNodeSources.execute(tx).map_err(database)?;
            upload.applied.deleted.extend(existing.into_keys());
            upload.applied.changed = true;
        }
    }
    let mut seen: HashMap<Provider, HashSet<String>> = HashMap::new();
    let mut reconciled = HashMap::new();
    let mut scans = BTreeMap::new();
    let mut requests = BTreeSet::new();
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
        if duplicate {
            continue;
        }
        let affected = upload.affected(&event)?;
        if affected.iter().any(|provider| scans.contains_key(provider)) {
            return Err(AppError::usage("provider data after silver scan report"));
        }
        dirty.extend(affected);
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
                        }
                    }
                    Batch::Conversations { records } => {
                        for record in records {
                            upload.conversation(record, input.snapshot)?;
                        }
                    }
                    Batch::Messages { records } => {
                        for record in records {
                            upload.message(record)?;
                        }
                    }
                }
            }
            Event::ReuseGold {
                conversation_id,
                fingerprint,
            } if input.reuse_gold => {
                valid_id(&conversation_id)?;
                if gold
                    .reusable
                    .get(&conversation_id)
                    .is_none_or(|hash| hash != &fingerprint)
                    || !requests.insert(conversation_id)
                {
                    return Err(conflict("shared gold reuse is unverified; retry indexing"));
                }
            }
            Event::Source(source) if !input.snapshot => {
                valid_id(&source.path_hash)?;
                if source.size_bytes < 0
                    || reconciled.contains_key(&source.provider)
                    || scans.contains_key(&source.provider)
                {
                    return Err(AppError::usage("invalid source checkpoint"));
                }
                upload.source(&source)?;
            }
            Event::Seen { provider, hashes } if !input.snapshot => {
                if hashes.len() > 128
                    || reconciled.contains_key(&provider)
                    || scans.contains_key(&provider)
                {
                    return Err(AppError::usage("invalid observed sources"));
                }
                seen.entry(provider).or_default().extend(hashes);
            }
            Event::Reconcile { provider, cutoff } if !input.snapshot => {
                if scans.contains_key(&provider) || reconciled.insert(provider, cutoff).is_some() {
                    return Err(AppError::usage("duplicate reconciliation"));
                }
                upload.reconcile(provider, cutoff, seen.get(&provider))?;
            }
            Event::Remove { id } if !input.snapshot => {
                valid_id(&id)?;
                let (m, c) = upload.remove(&id)?;
                upload.applied.removed += m;
                upload.applied.purged += c;
                upload.applied.changed |= c > 0;
            }
            Event::Scan(report) if !input.snapshot => {
                protocol::record_scan(&mut scans, report, &reconciled)?;
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
    if duplicate {
        // A resent revision changes no silver; it only records that the node synced.
        let mut applied = upload.applied;
        applied.head.last_synced_at = Some(now());
        put_head(tx, &applied.head)?;
        return Ok(applied);
    }
    if upload.missing_parents()? {
        return Err(AppError::usage("missing message context parent"));
    }
    let supplied = upload.applied.supplied.clone();
    for id in &supplied {
        let fingerprint = upload.fingerprint(id)?;
        sql::SetFingerprint::builder()
            .fingerprint(Some(&fingerprint))
            .id(id)
            .build()
            .execute(tx)
            .map_err(database)?;
        if requests.contains(id) {
            if gold.reusable.get(id) != Some(&fingerprint) {
                return Err(conflict("shared gold dependencies changed; retry indexing"));
            }
            if upload.vectors.contains(id) {
                return Err(AppError::usage(
                    "reused thread also supplies gold embeddings",
                ));
            }
        }
    }
    if requests.iter().any(|id| !supplied.contains(id)) {
        return Err(conflict("shared gold dependencies changed; retry indexing"));
    }
    upload.applied.reused = requests;
    if let Some(before) = before {
        if upload.canonical()? != before {
            return Err(conflict(
                "gold refresh cannot change silver at the same source revision",
            ));
        }
        upload.applied.changed = false;
    }
    let canonical = upload.canonical()?;
    let mut applied = upload.applied;
    let coverage = crate::shared::silver_coverage::update(
        &applied.head.silver_coverage,
        &scans,
        &dirty,
        tx,
        revision,
        input.snapshot,
    )?;
    let now = now();
    applied.head.silver_coverage = coverage;
    applied.head.silver_scans = scans.into_values().collect::<Vec<ScanReport>>();
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
    put_head(tx, &applied.head)?;
    Ok(applied)
}

/// Tombstone threads directly, outside a source upload.
pub(in crate::shared) fn forget(
    tx: &Transaction<'_>,
    identity: &Identity,
    instance: &str,
    revision: Option<i64>,
    ids: &[String],
) -> Result<(bool, Head, BTreeSet<String>), AppError> {
    valid_id(instance)?;
    if ids.len() > 10000 {
        return Err(AppError::usage("too many forget identities"));
    }
    let mut previous = head(tx)?;
    let accepted = previous.as_ref().map_or(0, |h| h.revision);
    if previous.as_ref().is_some_and(|h| h.instance != instance)
        || revision.is_some_and(|r| r <= accepted)
    {
        return Err(conflict("node identity or revision conflict"));
    }
    let mut upload = Upload {
        tx,
        identity,
        build: &Build::default(),
        applied: Applied {
            head: previous.take().unwrap_or(Head {
                instance: instance.to_owned(),
                revision: 0,
                version: String::new(),
                silver_revision: None,
                source_silver: None,
                silver_coverage: Vec::new(),
                silver_scans: Vec::new(),
                last_synced_at: None,
                last_updated_at: None,
            }),
            revision: 0,
            duplicate: false,
            changed: false,
            removed: 0,
            purged: 0,
            supplied: BTreeSet::new(),
            reused: BTreeSet::new(),
            deleted: BTreeSet::new(),
        },
        vectors: HashSet::new(),
    };
    let mut forgotten = false;
    for id in ids {
        forgotten |= upload.forget(id)?;
    }
    let canonical = upload.canonical()?;
    let mut applied = upload.applied;
    applied.head.revision = revision.unwrap_or(
        accepted
            .checked_add(1)
            .ok_or_else(|| conflict("node revision exhausted"))?,
    );
    // Direct forgets do not certify a provider scan.
    applied.head.source_silver = None;
    applied.head.silver_scans.clear();
    applied.head.silver_coverage.clear();
    applied.head.silver_revision = Some(crate::shared::silver_revision::State::next(
        applied.head.silver_revision.as_ref(),
        canonical,
    )?);
    applied.head.version = token()?;
    if forgotten {
        applied.head.last_updated_at = Some(now());
    }
    put_head(tx, &applied.head)?;
    Ok((forgotten, applied.head, applied.deleted))
}
