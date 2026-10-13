use super::{Storage, generated as sql};
use crate::{
    AppError,
    attachments::{self, Availability, Listing, Metadata, Retrieved},
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// A complete verification of one attachment cache: the directory's
/// modification time when it began and every digest silver then retained.
struct VerifiedCache {
    stamp: SystemTime,
    digests: BTreeSet<String>,
}

/// Process-local, so a watcher reads the archive's attachment metadata once
/// rather than on every pass. Adding a file changes the directory's time and
/// forces a full verification; removing one is also caught by re-checking the
/// known digests, which costs one stat each. A new process always verifies.
static VERIFIED_CACHES: Mutex<BTreeMap<PathBuf, VerifiedCache>> = Mutex::new(BTreeMap::new());

/// Whether a verification at this stamp still holds: `None` when it must rerun.
fn memoized_cache_missing(root: &Path, stamp: SystemTime) -> Option<bool> {
    let verified = VERIFIED_CACHES.lock().ok()?;
    let cache = verified.get(root).filter(|cache| cache.stamp == stamp)?;
    Some(
        cache
            .digests
            .iter()
            .any(|digest| !root.join(digest).is_file()),
    )
}

fn record_verified_cache(root: PathBuf, cache: VerifiedCache) {
    if let Ok(mut verified) = VERIFIED_CACHES.lock() {
        verified.insert(root, cache);
    }
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
            self.connection.execute_batch("UPDATE source_checkpoints SET interpretation_version=0; UPDATE native_chats SET source_fingerprint=''; UPDATE native_messages SET fingerprint='' WHERE storage_id IN (SELECT storage_id FROM objects WHERE origin='provider' AND attachments<>'[]');").map_err(AppError::database)?;
        }
        sql::SetAttachmentPolicy::builder()
            .fingerprint(&fingerprint)
            .build()
            .execute(&self.connection)
            .map_err(AppError::database)?;
        self.attachments_config = config;
        Ok(())
    }

    #[tracing::instrument(name = "verify_attachment_cache", skip_all, fields(memoized = tracing::field::Empty))]
    fn cached_attachments_missing(&self) -> Result<bool, AppError> {
        let database = self.connection.path().ok_or_else(|| {
            AppError::internal("attachment storage requires a file-backed database")
        })?;
        let root = attachments::directory(Path::new(database));
        // Stamp before reading, so a file added during verification forces the next one.
        let stamp = fs::metadata(&root).and_then(|m| m.modified()).ok();
        let memoized = stamp.and_then(|stamp| memoized_cache_missing(&root, stamp));
        tracing::Span::current().record("memoized", memoized.is_some());
        if let Some(missing) = memoized {
            return Ok(missing);
        }
        let digests = self.retained_attachment_digests()?;
        let missing = digests.iter().any(|digest| !root.join(digest).is_file());
        if let (false, Some(stamp)) = (missing, stamp) {
            record_verified_cache(root, VerifiedCache { stamp, digests });
        }
        Ok(missing)
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
            attachments::validate_metadata(&records)?;
            digests.extend(
                records
                    .into_iter()
                    .filter(|m| m.availability == Availability::Retained)
                    .filter_map(|m| m.digest),
            );
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
                "SELECT search_projection FROM objects WHERE origin='provider' AND object_role='entry' AND id=?1",
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

    #[test]
    fn repeated_refreshes_reuse_cache_verification_until_a_file_is_removed() {
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
        let config = attachments::Configuration {
            allowed_roots: vec![files],
            share_files: false,
        };
        let db = root.path().join("index.sqlite3");
        let mut writer = Storage::open_writer(&db).unwrap();
        writer.configure_attachments(config.clone()).unwrap();
        silver::index(&mut writer, &options).unwrap();
        let id = sql::AttachmentCandidates
            .query_many(&writer.connection)
            .unwrap()
            .remove(0)
            .id;
        let metadata = writer.list_attachments(&id).unwrap().attachments.remove(0);
        assert_eq!(metadata.availability, Availability::Retained);
        // The first verification reads silver; an unchanged cache reuses it.
        assert!(!configure_traced(&mut writer, &config));
        assert!(configure_traced(&mut writer, &config));
        let replayed = |writer: &Storage| {
            writer
                .connection
                .query_row(
                    "SELECT count(*) FROM source_checkpoints WHERE interpretation_version = 0",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        };
        assert_eq!(replayed(&writer), 0);
        std::fs::remove_file(attachments::directory(&db).join(metadata.digest.as_ref().unwrap()))
            .unwrap();
        // Removal is detected even if the directory time did not visibly change.
        configure_traced(&mut writer, &config);
        assert_eq!(replayed(&writer), 1, "a removed cached file forces replay");
        silver::index(&mut writer, &options).unwrap();
        assert!(writer.get_attachment(&id, &metadata.id).is_ok());
        writer.commit_writer().unwrap();
    }
}
