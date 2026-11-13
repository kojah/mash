use super::{Storage, generated as sql};
use crate::{
    AppError,
    attachments::{self, Availability, Listing, Metadata, Retrieved},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A complete verification of one database's attachment cache: at this silver
/// revision every retained digest is in `digests`, and each one was a file.
/// It lives beside the database, so writing it changes neither the revision
/// nor the cache directory. Reusing it re-checks each digest, one stat each.
#[derive(Deserialize, Serialize)]
struct VerifiedCache {
    instance: String,
    revision: i64,
    digests: BTreeSet<String>,
}

/// A verification this connection can carry forward across its own writes.
/// `bumps` is this connection's revision count when it was taken; when every
/// later revision is this connection's, its logged attachment writes are all
/// the retained digests that can have been added.
pub(super) struct PendingVerification {
    cache: VerifiedCache,
    bumps: i64,
}

/// Per-connection: logs this connection's attachment writes and counts its
/// silver revisions, so its own commits keep a verification current.
const WRITE_LOG: &str = "
CREATE TEMP TABLE IF NOT EXISTS attachment_writes(attachments TEXT NOT NULL);
CREATE TEMP TABLE IF NOT EXISTS attachment_revisions(
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1), bumps INTEGER NOT NULL);
INSERT OR IGNORE INTO temp.attachment_revisions VALUES (1, 0);
CREATE TEMP TRIGGER IF NOT EXISTS attachment_writes_insert AFTER INSERT ON main.messages
WHEN NEW.attachments<>'[]' BEGIN
    INSERT INTO temp.attachment_writes VALUES (NEW.attachments);
END;
CREATE TEMP TRIGGER IF NOT EXISTS attachment_writes_update AFTER UPDATE ON main.messages
WHEN NEW.attachments<>'[]' AND NEW.attachments IS NOT OLD.attachments BEGIN
    INSERT INTO temp.attachment_writes VALUES (NEW.attachments);
END;
CREATE TEMP TRIGGER IF NOT EXISTS attachment_revisions_update AFTER UPDATE ON main.silver_state
WHEN NEW.revision<>OLD.revision BEGIN
    UPDATE temp.attachment_revisions SET bumps = bumps + NEW.revision - OLD.revision;
END;";

fn verification_path(database: &Path) -> PathBuf {
    let mut path = database.as_os_str().to_owned();
    path.push(".attachments-verified.json");
    PathBuf::from(path)
}

fn read_verification(database: &Path) -> Option<VerifiedCache> {
    serde_json::from_slice(&fs::read(verification_path(database)).ok()?).ok()
}

/// Best effort: without the record the next process verifies from silver.
fn record_verification(database: &Path, cache: &VerifiedCache) {
    let path = verification_path(database);
    let Ok(bytes) = serde_json::to_vec(cache) else {
        return;
    };
    let temporary = path.with_extension("json.tmp");
    if fs::write(&temporary, bytes).is_ok() {
        let _ = fs::rename(&temporary, &path);
    }
}

/// A verification that holds once its writer's commit succeeds.
pub(super) struct VerifiedCacheRecord {
    database: PathBuf,
    cache: VerifiedCache,
}

impl VerifiedCacheRecord {
    pub(super) fn record(&self) {
        record_verification(&self.database, &self.cache);
    }
}

fn retained_digests(
    records: Vec<Metadata>,
    digests: &mut BTreeSet<String>,
) -> Result<(), AppError> {
    attachments::validate_metadata(&records)?;
    digests.extend(
        records
            .into_iter()
            .filter(|m| m.availability == Availability::Retained)
            .filter_map(|m| m.digest),
    );
    Ok(())
}

impl Storage {
    pub(crate) fn attachment_candidates(&self) -> Result<Vec<(String, String)>, AppError> {
        let mut statement = self
            .connection
            .prepare(sql::AttachmentCandidates::QUERY)
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([], sql::AttachmentCandidatesRow::from_row)
            .map_err(AppError::database)?;
        let mut result = Vec::new();
        for row in rows {
            let row = row.map_err(AppError::database)?;
            let records: Vec<Metadata> = serde_json::from_str(&row.attachments)
                .map_err(|_| AppError::database_data("invalid attachments"))?;
            for record in records {
                if record.availability == Availability::Retained {
                    result.push((row.id.clone(), record.id));
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn configure_attachments(
        &mut self,
        config: attachments::Configuration,
    ) -> Result<(), AppError> {
        let bytes = serde_json::to_vec(&config.allowed_roots)
            .map_err(|_| AppError::internal("serialize attachment roots"))?;
        let fingerprint = blake3::hash(&bytes).to_hex().to_string();
        if sql::AttachmentPolicy
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .is_some_and(|row| row.fingerprint != fingerprint)
            || self.cached_attachments_missing()?
        {
            // A root-policy change must replay sources even when history files are
            // unchanged. Keep embeddings when visible dialogue remains identical.
            self.connection.execute_batch("UPDATE source_checkpoints SET interpretation_version=0; UPDATE chats SET source_fingerprint=''; UPDATE messages SET fingerprint='' WHERE attachments<>'[]';").map_err(AppError::database)?;
        }
        sql::SetAttachmentPolicy::builder()
            .fingerprint(&fingerprint)
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;
        self.attachments_config = config;
        Ok(())
    }

    /// Whether a retained attachment is missing from the cache. Silver is read
    /// only when no verification covers the current revision; a missing file is
    /// always confirmed against silver, so a stale digest cannot force a replay.
    #[tracing::instrument(name = "verify_attachment_cache", skip_all, fields(memoized = tracing::field::Empty))]
    fn cached_attachments_missing(&mut self) -> Result<bool, AppError> {
        let database = PathBuf::from(self.connection.path().ok_or_else(|| {
            AppError::internal("attachment storage requires a file-backed database")
        })?);
        let root = attachments::directory(&database);
        self.connection
            .execute_batch(WRITE_LOG)
            .map_err(AppError::database)?;
        let known = if let Some(pending) = self.attachment_verification.take() {
            self.advance_verification(pending)?
        } else {
            let (instance, revision) = self.attachment_revision()?;
            read_verification(&database)
                .filter(|cache| cache.instance == instance && cache.revision == revision)
        };
        let memoized = known
            .as_ref()
            .is_some_and(|cache| cache.digests.iter().all(|d| root.join(d).is_file()));
        tracing::Span::current().record("memoized", memoized);
        let cache = match known {
            Some(cache) if memoized => cache,
            _ => {
                let (instance, revision) = self.attachment_revision()?;
                let digests = self.retained_attachment_digests()?;
                if digests.iter().any(|digest| !root.join(digest).is_file()) {
                    return Ok(true);
                }
                VerifiedCache {
                    instance,
                    revision,
                    digests,
                }
            }
        };
        // This connection's later writes extend the verification, and its
        // commit records it for the next process.
        self.connection
            .execute_batch("DELETE FROM temp.attachment_writes")
            .map_err(AppError::database)?;
        let bumps = self.attachment_bumps()?;
        self.attachment_verification = Some(PendingVerification { cache, bumps });
        Ok(false)
    }

    fn attachment_revision(&self) -> Result<(String, i64), AppError> {
        self.connection
            .query_row(
                "SELECT (SELECT instance FROM local_database_identity WHERE singleton=1),
                        (SELECT revision FROM silver_state WHERE singleton=1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(AppError::database)
    }

    fn attachment_bumps(&self) -> Result<i64, AppError> {
        self.connection
            .query_row(
                "SELECT bumps FROM temp.attachment_revisions WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::database)
    }

    /// The verification at the current revision, when this connection made
    /// every revision since it: then its logged writes hold every retained
    /// digest that can have been added. `None` when another writer committed.
    fn advance_verification(
        &self,
        pending: PendingVerification,
    ) -> Result<Option<VerifiedCache>, AppError> {
        let (instance, revision) = self.attachment_revision()?;
        let PendingVerification { mut cache, bumps } = pending;
        if instance != cache.instance
            || revision - cache.revision != self.attachment_bumps()? - bumps
        {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare("SELECT attachments FROM temp.attachment_writes")
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(AppError::database)?;
        for row in rows {
            let records: Vec<Metadata> = serde_json::from_str(&row.map_err(AppError::database)?)
                .map_err(|_| AppError::database_data("invalid attachments"))?;
            retained_digests(records, &mut cache.digests)?;
        }
        cache.revision = revision;
        Ok(Some(cache))
    }

    /// Before the writer commits: the verification its commit makes current,
    /// when every digest it holds is still a file.
    pub(super) fn verification_to_record(&mut self) -> Option<VerifiedCacheRecord> {
        let pending = self.attachment_verification.take()?;
        let cache = self.advance_verification(pending).ok()??;
        let database = PathBuf::from(self.connection.path()?);
        let root = attachments::directory(&database);
        cache
            .digests
            .iter()
            .all(|digest| root.join(digest).is_file())
            .then_some(VerifiedCacheRecord { database, cache })
    }

    /// Reads every native entry's attachment metadata: proportional to the archive.
    fn retained_attachment_digests(&self) -> Result<BTreeSet<String>, AppError> {
        let mut statement = self
            .connection
            .prepare(sql::AttachmentCandidates::QUERY)
            .map_err(AppError::database)?;
        let rows = statement
            .query_map([], sql::AttachmentCandidatesRow::from_row)
            .map_err(AppError::database)?;
        let mut digests = BTreeSet::new();
        for row in rows {
            let records: Vec<Metadata> =
                serde_json::from_str(&row.map_err(AppError::database)?.attachments)
                    .map_err(|_| AppError::database_data("invalid attachments"))?;
            retained_digests(records, &mut digests)?;
        }
        Ok(digests)
    }

    pub(super) fn retain_attachments(
        &self,
        message: &crate::silver::NormalizedMessage,
    ) -> Result<Vec<Metadata>, AppError> {
        let database = self.connection.path().ok_or_else(|| {
            AppError::internal("attachment storage requires a file-backed database")
        })?;
        attachments::retain(
            &attachments::directory(Path::new(database)),
            &self.attachments_config,
            &message.id,
            &message.attachments,
        )
    }

    pub(crate) fn list_attachments(&self, message_id: &str) -> Result<Listing, AppError> {
        let row = sql::MessageAttachments::builder()
            .id(message_id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
            .ok_or_else(|| AppError::usage("message not found"))?;
        let mut records: Vec<Metadata> = serde_json::from_str(&row.attachments)
            .map_err(|_| AppError::database_data("invalid attachment metadata"))?;
        attachments::validate_metadata(&records)?;
        if let Some(database) = self.connection.path() {
            let root = attachments::directory(Path::new(database));
            for record in &mut records {
                if record.availability == Availability::Retained
                    && record
                        .digest
                        .as_deref()
                        .is_none_or(|digest| !root.join(digest).is_file())
                {
                    record.availability = Availability::Missing;
                }
            }
        }
        Ok(Listing {
            message_id: message_id.into(),
            attachments: records,
        })
    }

    pub(crate) fn get_attachment(&self, message_id: &str, id: &str) -> Result<Retrieved, AppError> {
        let attachment = self
            .list_attachments(message_id)?
            .attachments
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| AppError::usage("attachment not found on this message"))?;
        let database = self.connection.path().ok_or_else(|| {
            AppError::internal("attachment storage requires a file-backed database")
        })?;
        let data = attachments::read(&attachments::directory(Path::new(database)), &attachment)?;
        Ok(Retrieved {
            message_id: message_id.into(),
            attachment,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider::{Provider, ProviderRoots},
        silver::{self, IndexOptions},
    };

    #[test]
    fn changing_attachment_roots_replays_unchanged_sources_and_preserves_visible_text() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("session.jsonl");
        let files = root.path().join("files");
        std::fs::create_dir(&files).unwrap();
        let image = files.join("photo.png");
        std::fs::write(&image, [137, 80, 78, 71]).unwrap();
        std::fs::write(&source, serde_json::to_string(&serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn","item":{"type":"UserMessage","id":"message","content":[{"type":"text","text":"real request"},{"type":"local_image","path":image}]}}})).unwrap()).unwrap();
        let options = IndexOptions {
            providers: ProviderRoots::from([(Provider::Codex, vec![root.path().to_path_buf()])]),
            roots_are_authoritative: true,
            since_days: None,
        };
        let db = root.path().join("index.sqlite3");
        let mut writer = Storage::open_writer(&db).unwrap();
        writer
            .configure_attachments(attachments::Configuration::default())
            .unwrap();
        silver::index(&mut writer, &options).unwrap();
        let id = sql::AttachmentCandidates
            .query_many(&writer.connection)
            .unwrap()
            .remove(0)
            .id;
        assert_eq!(
            writer.list_attachments(&id).unwrap().attachments[0].availability,
            Availability::NotAllowed
        );
        writer
            .configure_attachments(attachments::Configuration {
                allowed_roots: vec![files.clone()],
                share_files: false,
            })
            .unwrap();
        silver::index(&mut writer, &options).unwrap();
        assert_eq!(
            writer.list_attachments(&id).unwrap().attachments[0].availability,
            Availability::Retained
        );
        let visible: String = writer
            .connection
            .query_row(
                "SELECT search_projection FROM messages WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(visible, "real request");
        let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
        std::fs::remove_file(attachments::directory(&db).join(metadata.digest.as_ref().unwrap()))
            .unwrap();
        assert_eq!(
            writer.list_attachments(&id).unwrap().attachments[0].availability,
            Availability::Missing
        );
        writer
            .configure_attachments(attachments::Configuration {
                allowed_roots: vec![files],
                share_files: false,
            })
            .unwrap();
        silver::index(&mut writer, &options).unwrap();
        assert!(writer.get_attachment(&id, &metadata.id).is_ok());
        writer.commit_writer().unwrap();
    }

    /// Whether configuring reused the process's earlier cache verification.
    fn configure_traced(writer: &mut Storage, config: &attachments::Configuration) -> bool {
        let (bytes, dispatcher) = crate::observability::test_support::capture();
        tracing::dispatcher::with_default(&dispatcher, || {
            writer.configure_attachments(config.clone())
        })
        .unwrap();
        let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        output.lines().any(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            event["span"]["name"] == "verify_attachment_cache"
                && event["span"]["memoized"] == serde_json::Value::Bool(true)
        })
    }

    /// One Codex chat whose message retains one allowed image.
    struct Fixture {
        _root: tempfile::TempDir,
        db: PathBuf,
        options: IndexOptions,
        config: attachments::Configuration,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("session.jsonl");
            let files = root.path().join("files");
            std::fs::create_dir(&files).unwrap();
            let image = files.join("photo.png");
            std::fs::write(&image, [137, 80, 78, 71]).unwrap();
            std::fs::write(&source, serde_json::to_string(&serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","turn_id":"turn","item":{"type":"UserMessage","id":"message","content":[{"type":"text","text":"real request"},{"type":"local_image","path":image}]}}})).unwrap()).unwrap();
            Self {
                options: IndexOptions {
                    providers: ProviderRoots::from([(
                        Provider::Codex,
                        vec![root.path().to_path_buf()],
                    )]),
                    roots_are_authoritative: true,
                    since_days: None,
                },
                config: attachments::Configuration {
                    allowed_roots: vec![files],
                    share_files: false,
                },
                db: root.path().join("index.sqlite3"),
                _root: root,
            }
        }

        /// The retained message and its attachment after a first committed index.
        fn indexed(&self) -> (String, Metadata) {
            let mut writer = Storage::open_writer(&self.db).unwrap();
            writer.configure_attachments(self.config.clone()).unwrap();
            silver::index(&mut writer, &self.options).unwrap();
            let id = sql::AttachmentCandidates
                .query_many(&writer.connection)
                .unwrap()
                .remove(0)
                .id;
            let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
            assert_eq!(metadata.availability, Availability::Retained);
            writer.commit_writer().unwrap();
            (id, metadata)
        }

        fn remove_cached(&self, metadata: &Metadata) {
            std::fs::remove_file(
                attachments::directory(&self.db).join(metadata.digest.as_ref().unwrap()),
            )
            .unwrap();
        }
    }

    fn replayed(writer: &Storage) -> i64 {
        writer
            .connection
            .query_row(
                "SELECT count(*) FROM source_checkpoints WHERE interpretation_version = 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    }

    #[test]
    fn repeated_refreshes_reuse_cache_verification_until_a_file_is_removed() {
        let fixture = Fixture::new();
        let (config, options, db) = (&fixture.config, &fixture.options, &fixture.db);
        let mut writer = Storage::open_writer(db).unwrap();
        // The first verification reads silver; the writer's own indexing,
        // including the file it retains, extends it rather than forcing another.
        assert!(!configure_traced(&mut writer, config));
        silver::index(&mut writer, options).unwrap();
        let id = sql::AttachmentCandidates
            .query_many(&writer.connection)
            .unwrap()
            .remove(0)
            .id;
        let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
        assert_eq!(metadata.availability, Availability::Retained);
        assert!(configure_traced(&mut writer, config));
        assert!(configure_traced(&mut writer, config));
        assert_eq!(replayed(&writer), 0);
        fixture.remove_cached(&metadata);
        // Removal is detected even if the directory time did not visibly change.
        assert!(!configure_traced(&mut writer, config));
        assert_eq!(replayed(&writer), 1, "a removed cached file forces replay");
        silver::index(&mut writer, options).unwrap();
        assert!(writer.get_attachment(&id, &metadata.id).is_ok());
        writer.commit_writer().unwrap();
    }

    #[test]
    fn a_new_process_reuses_the_committed_verification_and_detects_removal() {
        let fixture = Fixture::new();
        let (id, metadata) = fixture.indexed();
        // A fresh writer has no verification of its own: it reads the record
        // its predecessor's commit wrote instead of the archive.
        let mut writer = Storage::open_writer(&fixture.db).unwrap();
        assert!(writer.attachment_verification.is_none());
        assert!(configure_traced(&mut writer, &fixture.config));
        silver::index(&mut writer, &fixture.options).unwrap();
        writer.commit_writer().unwrap();
        drop(writer);
        let mut writer = Storage::open_writer(&fixture.db).unwrap();
        assert!(configure_traced(&mut writer, &fixture.config));
        assert_eq!(replayed(&writer), 0);
        writer.commit_writer().unwrap();
        drop(writer);

        fixture.remove_cached(&metadata);
        let mut writer = Storage::open_writer(&fixture.db).unwrap();
        assert!(!configure_traced(&mut writer, &fixture.config));
        assert_eq!(replayed(&writer), 1, "a removed cached file forces replay");
        silver::index(&mut writer, &fixture.options).unwrap();
        assert!(writer.get_attachment(&id, &metadata.id).is_ok());
        writer.commit_writer().unwrap();
    }

    #[test]
    fn another_writers_attachment_change_invalidates_the_recorded_verification() {
        let fixture = Fixture::new();
        let (id, metadata) = fixture.indexed();
        // Another connection retains a digest the cache never held.
        let connection = rusqlite::Connection::open(&fixture.db).unwrap();
        crate::storage::packed::register(&connection).unwrap();
        let mut phantom = metadata.clone();
        phantom.id = "f".repeat(64);
        phantom.digest = Some("e".repeat(64));
        connection
            .execute(
                "UPDATE messages SET attachments=?1 WHERE id=?2",
                [serde_json::to_string(&[metadata, phantom]).unwrap(), id],
            )
            .unwrap();
        drop(connection);
        let mut writer = Storage::open_writer(&fixture.db).unwrap();
        assert!(!configure_traced(&mut writer, &fixture.config));
        assert_eq!(
            replayed(&writer),
            1,
            "an unverified retained digest forces replay"
        );
    }

    #[test]
    fn an_uncommitted_verification_is_never_recorded() {
        let fixture = Fixture::new();
        fixture.indexed();
        let record = verification_path(&fixture.db);
        std::fs::remove_file(&record).unwrap();
        let mut writer = Storage::open_writer(&fixture.db).unwrap();
        assert!(!configure_traced(&mut writer, &fixture.config));
        silver::index(&mut writer, &fixture.options).unwrap();
        drop(writer);
        assert!(!record.exists(), "a rolled-back writer records nothing");
    }
}
