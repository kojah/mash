//! Disk-backed pending-text deduplication with bounded wave and ID reads.
use super::Storage;
use crate::AppError;
use rusqlite::{Connection, params};

pub(crate) const MESSAGE_PAGE_SIZE: usize = 1024;

pub(crate) struct PlannedText {
    pub(crate) group_id: i64,
    pub(crate) content: String,
}

pub(crate) struct EmbeddingPlan {
    // Close SQLite before deleting its private temporary directory.
    connection: Connection,
    _directory: tempfile::TempDir,
    position: i64,
    pub(crate) total_vectors: u64,
}

impl EmbeddingPlan {
    pub(crate) fn new(storage: &Storage) -> Result<Self, AppError> {
        let directory = tempfile::Builder::new()
            .prefix("mash-embedding-plan-")
            .tempdir()
            .map_err(AppError::io)?;
        let mut connection =
            Connection::open(directory.path().join("plan.sqlite3")).map_err(AppError::database)?;
        connection
            .execute_batch(
                "PRAGMA cache_size = -8192;
             PRAGMA temp_store = FILE;
             CREATE TABLE groups (
                 group_id INTEGER PRIMARY KEY,
                 digest BLOB NOT NULL UNIQUE,
                 byte_length INTEGER NOT NULL,
                 content TEXT NOT NULL
             );
             CREATE TABLE members (
                 group_id INTEGER NOT NULL,
                 message_id TEXT NOT NULL,
                 PRIMARY KEY (group_id, message_id)
             ) WITHOUT ROWID;
             CREATE TABLE ordered_groups (
                 position INTEGER PRIMARY KEY,
                 group_id INTEGER NOT NULL
             );",
            )
            .map_err(AppError::database)?;
        let total_vectors = Self::populate(&mut connection, storage)?;
        Ok(Self {
            connection,
            _directory: directory,
            position: 0,
            total_vectors,
        })
    }

    fn populate(connection: &mut Connection, storage: &Storage) -> Result<u64, AppError> {
        let transaction = connection.transaction().map_err(AppError::database)?;
        let mut total = 0_u64;
        {
            let mut source = storage
                .connection
                .prepare(
                    "SELECT messages.id, COALESCE(messages.search_projection, messages.content)
                   FROM pending_embeddings
                   JOIN messages ON messages.id = pending_embeddings.message_id
                  ORDER BY pending_embeddings.message_id",
                )
                .map_err(AppError::database)?;
            let mut rows = source.query([]).map_err(AppError::database)?;
            let mut insert = transaction
                .prepare(
                    "INSERT INTO groups(digest, byte_length, content) VALUES (?1, ?2, ?3)
                 ON CONFLICT(digest) DO NOTHING",
                )
                .map_err(AppError::database)?;
            let mut existing = transaction
                .prepare("SELECT group_id, content FROM groups WHERE digest = ?1")
                .map_err(AppError::database)?;
            let mut member = transaction
                .prepare("INSERT INTO members(group_id, message_id) VALUES (?1, ?2)")
                .map_err(AppError::database)?;
            while let Some(row) = rows.next().map_err(AppError::database)? {
                let id: String = row.get(0).map_err(AppError::database)?;
                let content: String = row.get(1).map_err(AppError::database)?;
                let digest = blake3::hash(content.as_bytes());
                let length = i64::try_from(content.len())
                    .map_err(|_| AppError::internal("embedding text is too large"))?;
                let inserted = insert
                    .execute(params![digest.as_bytes().as_slice(), length, content])
                    .map_err(AppError::database)?;
                let group_id = if inserted == 1 {
                    transaction.last_insert_rowid()
                } else {
                    let (group_id, previous): (i64, String) = existing
                        .query_row([digest.as_bytes().as_slice()], |row| {
                            Ok((row.get(0)?, row.get(1)?))
                        })
                        .map_err(AppError::database)?;
                    if previous != content {
                        return Err(AppError::internal("embedding plan text digest collision"));
                    }
                    group_id
                };
                member
                    .execute(params![group_id, id])
                    .map_err(AppError::database)?;
                total = total
                    .checked_add(1)
                    .ok_or_else(|| AppError::internal("too many embeddings"))?;
            }
        }
        // Materialize the order once. SQLite can spill this sort to disk; paging
        // subsequently follows integer keys rather than re-sorting all texts.
        transaction
            .execute_batch(
                "INSERT INTO ordered_groups(group_id)
             SELECT group_id FROM groups ORDER BY byte_length, content COLLATE BINARY",
            )
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
        Ok(total)
    }

    pub(crate) fn next_wave(&mut self, limit: usize) -> Result<Vec<PlannedText>, AppError> {
        let limit =
            i64::try_from(limit).map_err(|_| AppError::internal("embedding wave is too large"))?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT o.position, g.group_id, g.content FROM ordered_groups o
             JOIN groups g ON g.group_id = o.group_id
             WHERE o.position > ?1 ORDER BY o.position LIMIT ?2",
            )
            .map_err(AppError::database)?;
        let mut rows = statement
            .query(params![self.position, limit])
            .map_err(AppError::database)?;
        let mut wave = Vec::new();
        while let Some(row) = rows.next().map_err(AppError::database)? {
            self.position = row.get(0).map_err(AppError::database)?;
            wave.push(PlannedText {
                group_id: row.get(1).map_err(AppError::database)?,
                content: row.get(2).map_err(AppError::database)?,
            });
        }
        Ok(wave)
    }

    pub(crate) fn message_ids(
        &self,
        group: i64,
        after: Option<&str>,
    ) -> Result<Vec<String>, AppError> {
        let sql = if after.is_some() {
            "SELECT message_id FROM members
             WHERE group_id = ?1 AND message_id > ?2
             ORDER BY message_id LIMIT ?3"
        } else {
            "SELECT message_id FROM members
             WHERE group_id = ?1 AND ?2 IS NULL
             ORDER BY message_id LIMIT ?3"
        };
        let limit = i64::try_from(MESSAGE_PAGE_SIZE)
            .map_err(|_| AppError::internal("embedding message page is too large"))?;
        let mut statement = self.connection.prepare(sql).map_err(AppError::database)?;
        let rows = statement
            .query_map(params![group, after, limit], |row| row.get(0))
            .map_err(AppError::database)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)
    }
}

#[cfg(test)]
mod tests;
