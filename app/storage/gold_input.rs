//! Consistent canonical input for local Qdrant projection. No SQLite gold reads.
use super::{Counts, Storage};
use crate::{
    AppError,
    views::documents::{ChatRecord, MessageRecord},
};
use std::{
    collections::BTreeMap,
    io::{BufRead, Seek, Write},
    path::Path,
};

pub(crate) struct Chat {
    pub(crate) record: ChatRecord,
    pub(crate) fingerprint: String,
    offset: u64,
    messages: usize,
}

pub(crate) struct Input {
    pub(crate) instance: String,
    pub(crate) certificate: crate::silver::certificate::Certificate,
    pub(crate) counts: Counts,
    pub(crate) chats: BTreeMap<String, Chat>,
    messages: tempfile::NamedTempFile,
}

impl Input {
    pub(crate) fn capture(path: &Path) -> Result<Self, AppError> {
        use crate::shared::protocol::{Batch, BatchKind};
        let storage = Storage::open_silver(path)?;
        let mut messages = tempfile::NamedTempFile::new().map_err(AppError::io)?;
        let mut writer = std::io::BufWriter::new(messages.as_file_mut());
        let mut chats = BTreeMap::<String, Chat>::new();
        let mut hashes = BTreeMap::<String, blake3::Hasher>::new();
        let mut offset = 0;
        let mut captured = None;
        storage.with_publication_snapshot(|snapshot| {
            snapshot.visit_selected(BatchKind::Chats, false, None, |batch| {
                if let Batch::Chats { records } = batch {
                    for record in records {
                        let mut hash = blake3::Hasher::new();
                        hash.update(
                            &serde_json::to_vec(&record)
                                .map_err(|_| AppError::internal("encode gold input"))?,
                        );
                        hashes.insert(record.id.clone(), hash);
                        chats.insert(
                            record.id.clone(),
                            Chat {
                                record,
                                fingerprint: String::new(),
                                offset: 0,
                                messages: 0,
                            },
                        );
                    }
                }
                Ok(())
            })?;
            snapshot.visit_selected(BatchKind::Messages, true, None, |batch| {
                if let Batch::Messages { records } = batch {
                    for record in records {
                        let bytes = serde_json::to_vec(&record)
                            .map_err(|_| AppError::internal("encode gold input"))?;
                        let chat = chats
                            .get_mut(&record.chat_id)
                            .ok_or_else(|| AppError::database_data("gold input has no chat"))?;
                        if chat.messages == 0 {
                            chat.offset = offset;
                        }
                        chat.messages += 1;
                        hashes
                            .get_mut(&record.chat_id)
                            .ok_or_else(|| AppError::internal("missing chat digest"))?
                            .update(&bytes);
                        writer
                            .write_all(&bytes)
                            .and_then(|()| writer.write_all(b"\n"))
                            .map_err(AppError::io)?;
                        offset += bytes.len() as u64 + 1;
                    }
                }
                Ok(())
            })?;
            captured = Some((
                storage.local_gold_identity(path)?,
                snapshot.source_silver.clone(),
                storage.silver_counts()?,
            ));
            Ok(())
        })?;
        writer.flush().map_err(AppError::io)?;
        drop(writer);
        for (id, hash) in hashes {
            chats
                .get_mut(&id)
                .ok_or_else(|| AppError::internal("missing captured chat"))?
                .fingerprint = hash.finalize().to_hex().to_string();
        }
        let (instance, certificate, counts) =
            captured.ok_or_else(|| AppError::internal("missing gold input"))?;
        Ok(Self {
            instance,
            certificate,
            counts,
            chats,
            messages,
        })
    }

    pub(crate) fn read_chat(&self, id: &str) -> Result<Vec<MessageRecord>, AppError> {
        let chat = self
            .chats
            .get(id)
            .ok_or_else(|| AppError::internal("missing captured chat"))?;
        let mut file = self.messages.reopen().map_err(AppError::io)?;
        file.seek(std::io::SeekFrom::Start(chat.offset))
            .map_err(AppError::io)?;
        let records = std::io::BufReader::new(file)
            .lines()
            .take(chat.messages)
            .map(|line| {
                serde_json::from_str(&line.map_err(AppError::io)?)
                    .map_err(|_| AppError::internal("invalid gold input"))
            })
            .collect::<Result<Vec<MessageRecord>, AppError>>()?;
        if records.len() != chat.messages || records.iter().any(|m| m.chat_id != id) {
            return Err(AppError::internal("incomplete gold input"));
        }
        Ok(records)
    }
}

impl Storage {
    pub(crate) fn local_gold_state(
        &self,
        path: &Path,
    ) -> Result<(String, crate::silver::certificate::Certificate), AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let state = (self.local_gold_identity(path)?, self.silver_certificate()?);
        transaction.commit().map_err(AppError::database)?;
        Ok(state)
    }

    /// Copies at another path and imports must not inherit gold certification.
    /// Resolve symlinks so aliases of the same database retain one namespace.
    pub(crate) fn local_gold_identity(&self, path: &Path) -> Result<String, AppError> {
        let instance: String = self
            .connection
            .query_row(
                "SELECT instance FROM local_database_identity WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        let canonical = std::fs::canonicalize(path).map_err(AppError::io)?;
        let mut hash = blake3::Hasher::new();
        hash.update(instance.as_bytes());
        hash.update(&[0]);
        hash.update(canonical.as_os_str().as_encoded_bytes());
        Ok(hash.finalize().to_hex().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(path: &Path) -> Storage {
        let connection = rusqlite::Connection::open(path).unwrap();
        crate::storage::schema::initialize_silver(&connection).unwrap();
        Storage::from_silver(connection)
    }

    #[test]
    fn local_gold_identity_isolates_copies_and_same_path_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silver.db");
        let storage = create(&path);
        let original = storage.local_gold_identity(&path).unwrap();
        storage
            .connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        let copy = directory.path().join("copy.db");
        std::fs::copy(&path, &copy).unwrap();
        let copied = Storage::open_silver(&copy).unwrap();
        assert_ne!(original, copied.local_gold_identity(&copy).unwrap());
        drop(storage);
        std::fs::remove_file(&path).unwrap();
        let replacement = create(&path);
        assert_ne!(original, replacement.local_gold_identity(&path).unwrap());
    }

    #[test]
    fn local_gold_identity_is_not_preserved_by_keep_identity_archive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silver.db");
        let storage = create(&path);
        let original = storage.local_gold_identity(&path).unwrap();
        let archive = directory.path().join("archive.jsonl");
        crate::storage::export::write(&path, &archive).unwrap();
        drop(storage);
        std::fs::remove_file(&path).unwrap();
        crate::storage::export::restore(&archive, &path, true).unwrap();
        let restored = Storage::open_silver(&path).unwrap();
        assert_ne!(original, restored.local_gold_identity(&path).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn local_gold_identity_resolves_symlink_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silver.db");
        let storage = create(&path);
        let alias = directory.path().join("alias.db");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert_eq!(
            storage.local_gold_identity(&path).unwrap(),
            storage.local_gold_identity(&alias).unwrap()
        );
    }
}
