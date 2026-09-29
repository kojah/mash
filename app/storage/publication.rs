use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use super::Storage;
use crate::AppError;

pub(crate) struct Snapshot<'a> {
    connection: &'a Connection,
    pub(crate) instance: String,
    pub(crate) revision: i64,
}

pub(crate) struct DirectOwner {
    pub(crate) instance: String,
    pub(crate) revision: i64,
    pub(crate) scope: String,
    pub(crate) forgets: Vec<String>,
}

impl Snapshot<'_> {
    /// Bound each network batch without materializing the complete local history.
    pub(crate) fn visit(
        &self,
        kind: &str,
        mut accept: impl FnMut(&[Value]) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let sql = match kind {
            "conversations" => {
                "SELECT id, provider, title, created_at, updated_at FROM conversations ORDER BY id"
            }
            "messages" => {
                "SELECT m.id, m.conversation_id, m.ordinal, m.role, m.content,
                COALESCE(m.search_projection, m.content), m.created_at, e.generation, e.vector
                FROM messages m LEFT JOIN message_embeddings e ON e.message_id = m.id ORDER BY m.id"
            }
            "tombstones" => {
                "SELECT provider, conversation_id FROM tombstones ORDER BY provider, conversation_id"
            }
            _ => return Err(AppError::internal("unknown publication record kind")),
        };
        let mut statement = self.connection.prepare(sql).map_err(AppError::database)?;
        let mut rows = statement.query([]).map_err(AppError::database)?;
        let mut batch = Vec::new();
        let mut bytes = 0;
        while let Some(row) = rows.next().map_err(AppError::database)? {
            let value = match kind {
                "conversations" => json!({
                    "id": row.get::<_, String>(0).map_err(AppError::database)?,
                    "provider": row.get::<_, String>(1).map_err(AppError::database)?,
                    "title": row.get::<_, Option<String>>(2).map_err(AppError::database)?,
                    "created_at": row.get::<_, Option<i64>>(3).map_err(AppError::database)?,
                    "updated_at": row.get::<_, Option<i64>>(4).map_err(AppError::database)?,
                }),
                "messages" => {
                    let local_generation = row
                        .get::<_, Option<String>>(7)
                        .map_err(AppError::database)?;
                    let generation = (local_generation.as_deref()
                        == Some(crate::semantic::embedding_generation()))
                    .then(|| crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned())
                    .or_else(|| {
                        local_generation.filter(|g| {
                            g.starts_with("remote:")
                                || g == crate::semantic::SHARED_EMBEDDING_GENERATION
                        })
                    });
                    let vector = row
                        .get::<_, Option<Vec<u8>>>(8)
                        .map_err(AppError::database)?
                        .map(|bytes| {
                            bytes
                                .into_iter()
                                .map(|v| f64::from(v.cast_signed()))
                                .collect::<Vec<_>>()
                        });
                    json!({
                        "id": row.get::<_, String>(0).map_err(AppError::database)?,
                        "conversation_id": row.get::<_, String>(1).map_err(AppError::database)?,
                        "ordinal": row.get::<_, i64>(2).map_err(AppError::database)?,
                        "role": row.get::<_, String>(3).map_err(AppError::database)?,
                        "content": row.get::<_, String>(4).map_err(AppError::database)?,
                        "projection": row.get::<_, String>(5).map_err(AppError::database)?,
                        "created_at": row.get::<_, Option<i64>>(6).map_err(AppError::database)?,
                        "generation": generation,
                        "vector": vector,
                    })
                }
                _ => json!({
                    "provider": row.get::<_, String>(0).map_err(AppError::database)?,
                    "conversation_id": row.get::<_, String>(1).map_err(AppError::database)?,
                }),
            };
            bytes += value.to_string().len();
            batch.push(value);
            if batch.len() >= 128 || bytes >= 1024 * 1024 {
                accept(&batch)?;
                batch.clear();
                bytes = 0;
            }
        }
        if !batch.is_empty() {
            accept(&batch)?;
        }
        Ok(())
    }
}

impl Storage {
    pub(crate) fn queue_shared_forget(&self, scope: &str, id: &str) -> Result<(), AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO shared_forgets VALUES (?1,?2)",
                (scope, id),
            )
            .map_err(AppError::database)?;
        transaction
            .execute(
                "UPDATE shared_publication SET revision=revision+1 WHERE singleton=1",
                [],
            )
            .map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)
    }

    pub(crate) fn direct_owner(&self, scope: &str) -> Result<DirectOwner, AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        transaction
            .execute(
                "UPDATE shared_publication SET revision=revision+1 WHERE singleton=1",
                [],
            )
            .map_err(AppError::database)?;
        let (instance, revision) = transaction
            .query_row(
                "SELECT instance,revision FROM shared_publication WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(AppError::database)?;
        let forgets = transaction.prepare("SELECT conversation_id FROM shared_forgets WHERE scope=?1 ORDER BY conversation_id").map_err(AppError::database)?
            .query_map([scope], |r| r.get(0)).map_err(AppError::database)?.collect::<Result<Vec<_>, _>>().map_err(AppError::database)?;
        transaction.commit().map_err(AppError::database)?;
        Ok(DirectOwner {
            instance,
            revision,
            scope: scope.to_owned(),
            forgets,
        })
    }

    pub(crate) fn acknowledge_direct(
        &self,
        target: &str,
        owner: &DirectOwner,
        indexed: bool,
    ) -> Result<(), AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let revision: i64 = transaction
            .query_row(
                "SELECT revision FROM shared_publication WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .map_err(AppError::database)?;
        // A concurrent mutation stays pending; never erase a newer forget intent.
        if revision == owner.revision {
            for id in &owner.forgets {
                transaction
                    .execute(
                        "DELETE FROM shared_forgets WHERE scope=?1 AND conversation_id=?2",
                        (&owner.scope, id),
                    )
                    .map_err(AppError::database)?;
            }
            if indexed {
                transaction.execute("UPDATE shared_publication SET target=?1,acknowledged=revision WHERE singleton=1", [target]).map_err(AppError::database)?;
            }
        }
        transaction.commit().map_err(AppError::database)
    }

    pub(crate) fn with_publication_snapshot(
        &self,
        publish: impl FnOnce(&Snapshot<'_>) -> Result<(), AppError>,
    ) -> Result<i64, AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        let (instance, revision) = transaction
            .query_row(
                "SELECT instance, revision FROM shared_publication WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(AppError::database)?;
        publish(&Snapshot {
            connection: &transaction,
            instance,
            revision,
        })?;
        transaction.commit().map_err(AppError::database)?;
        Ok(revision)
    }

    pub(crate) fn acknowledge_publication(
        &self,
        target: &str,
        revision: i64,
    ) -> Result<(), AppError> {
        self.connection.execute(
            "UPDATE shared_publication SET target = ?1, acknowledged = ?2 WHERE singleton = 1 AND revision = ?2",
            (target, revision),
        ).map_err(AppError::database)?;
        Ok(())
    }

    pub(crate) fn publication_is_pending(
        path: &std::path::Path,
        target: &str,
    ) -> Result<bool, AppError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database)?;
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'shared_publication')",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        if !exists {
            return Ok(true);
        }
        connection.query_row(
            "SELECT revision <> acknowledged OR target <> ?1 FROM shared_publication WHERE singleton = 1",
            [target], |row| row.get(0),
        ).map_err(AppError::database)
    }
}
