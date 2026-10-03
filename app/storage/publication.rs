use crate::provider::Provider;
use crate::shared::protocol::{Batch, BatchKind, ConversationRecord, MessageRecord, Tombstone};
use rusqlite::{Connection, OpenFlags};

use super::Storage;
use crate::AppError;

pub(crate) struct Snapshot<'a> {
    pub(crate) source_silver: crate::silver::certificate::Certificate,
    connection: &'a Connection,
    pub(crate) instance: String,
    pub(crate) revision: i64,
}

pub(crate) struct RoutedOwner {
    pub(crate) instance: String,
    pub(crate) revision: i64,
    pub(crate) scope: String,
    pub(crate) forgets: Vec<String>,
}

impl Snapshot<'_> {
    pub(crate) fn claude_sources(
        &self,
    ) -> Result<Vec<super::claude_sources::ClaudeSourceRef>, AppError> {
        super::claude_sources::claude_references(self.connection)
    }

    pub(crate) fn claude_thread(
        &self,
        id: &str,
        sources: &std::collections::BTreeSet<String>,
    ) -> Result<Option<crate::silver::Conversation>, AppError> {
        super::claude_sources::claude_thread(self.connection, id, Some(sources))
    }
    /// Bound each network batch without materializing the complete local history.
    pub(crate) fn visit(
        &self,
        kind: BatchKind,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.visit_ordered(kind, false, accept)
    }

    pub(crate) fn visit_messages_by_thread(
        &self,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.visit_ordered(BatchKind::Messages, true, accept)
    }

    fn visit_ordered(
        &self,
        kind: BatchKind,
        by_thread: bool,
        mut accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let sql = match kind {
            BatchKind::Conversations => {
                "SELECT c.id,c.provider,c.title,c.created_at,c.updated_at,c.project,c.repository,e.evidence
                 FROM conversations c LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id ORDER BY c.id"
            }
            BatchKind::Messages if by_thread => {
                "SELECT m.id, m.conversation_id, m.ordinal, m.role, m.content,
                COALESCE(m.search_projection, m.content), m.created_at, NULL, NULL, m.context_parent
                FROM messages m ORDER BY m.conversation_id, m.ordinal, m.id"
            }
            BatchKind::Messages => {
                "SELECT m.id, m.conversation_id, m.ordinal, m.role, m.content,
                COALESCE(m.search_projection, m.content), m.created_at, e.generation, e.vector, m.context_parent
                FROM messages m LEFT JOIN message_embeddings e ON e.message_id = m.id ORDER BY m.id"
            }
            BatchKind::Tombstones => {
                "SELECT provider, conversation_id FROM tombstones ORDER BY provider, conversation_id"
            }
        };
        let mut statement = self.connection.prepare(sql).map_err(AppError::database)?;
        let mut rows = statement.query([]).map_err(AppError::database)?;
        let mut batch = match kind {
            BatchKind::Conversations => Batch::Conversations {
                records: Vec::new(),
            },
            BatchKind::Messages => Batch::Messages {
                records: Vec::new(),
            },
            BatchKind::Tombstones => Batch::Tombstones {
                records: Vec::new(),
            },
        };
        let mut bytes = 0;
        while let Some(row) = rows.next().map_err(AppError::database)? {
            let size = match &mut batch {
                Batch::Conversations { records } => {
                    let record = ConversationRecord {
                        enrichment: row
                            .get::<_, Option<String>>(7)
                            .map_err(AppError::database)?
                            .map(|json| super::enrichment::decode(&json))
                            .transpose()?,
                        id: row.get::<_, String>(0).map_err(AppError::database)?,
                        provider: row.get::<_, Provider>(1).map_err(AppError::database)?,
                        title: row
                            .get::<_, Option<String>>(2)
                            .map_err(AppError::database)?,
                        created_at: row.get::<_, Option<i64>>(3).map_err(AppError::database)?,
                        updated_at: row.get::<_, Option<i64>>(4).map_err(AppError::database)?,
                        project: row
                            .get::<_, Option<String>>(5)
                            .map_err(AppError::database)?,
                        repository: row
                            .get::<_, Option<String>>(6)
                            .map_err(AppError::database)?,
                    };
                    let size = serde_json::to_vec(&record)
                        .map_err(|_| AppError::internal("serialize index record"))?
                        .len();
                    records.push(record);
                    size
                }
                Batch::Messages { records } => {
                    let record = message_record(row)?;
                    let size = serde_json::to_vec(&record)
                        .map_err(|_| AppError::internal("serialize index record"))?
                        .len();
                    records.push(record);
                    size
                }
                Batch::Tombstones { records } => {
                    let record = Tombstone {
                        provider: row.get::<_, Provider>(0).map_err(AppError::database)?,
                        conversation_id: row.get::<_, String>(1).map_err(AppError::database)?,
                    };
                    let size = serde_json::to_vec(&record)
                        .map_err(|_| AppError::internal("serialize index record"))?
                        .len();
                    records.push(record);
                    size
                }
            };
            bytes += size;
            if batch.len() >= 128 || bytes >= 1024 * 1024 {
                accept(batch.take())?;
                bytes = 0;
            }
        }
        if batch.len() > 0 {
            accept(batch)?;
        }
        Ok(())
    }
}

fn message_record(row: &rusqlite::Row<'_>) -> Result<MessageRecord, AppError> {
    let local_generation = row
        .get::<_, Option<String>>(7)
        .map_err(AppError::database)?;
    let generation = (local_generation.as_deref() == Some(crate::semantic::embedding_generation()))
        .then(|| crate::semantic::SHARED_EMBEDDING_GENERATION.to_owned())
        .or_else(|| {
            local_generation.filter(|g| {
                g.starts_with("remote:") || g == crate::semantic::SHARED_EMBEDDING_GENERATION
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
    Ok(MessageRecord {
        id: row.get::<_, String>(0).map_err(AppError::database)?,
        conversation_id: row.get::<_, String>(1).map_err(AppError::database)?,
        ordinal: row.get::<_, i64>(2).map_err(AppError::database)?,
        context: crate::message_context::MessageContext::from_parent(
            row.get("context_parent").map_err(AppError::database)?,
        )?,
        role: row
            .get::<_, crate::role::Role>(3)
            .map_err(AppError::database)?,
        content: row.get::<_, String>(4).map_err(AppError::database)?,
        projection: row.get::<_, String>(5).map_err(AppError::database)?,
        created_at: row.get::<_, Option<i64>>(6).map_err(AppError::database)?,
        generation,
        vector,
    })
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

    pub(crate) fn routed_owner(&self, scope: &str) -> Result<RoutedOwner, AppError> {
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
        Ok(RoutedOwner {
            instance,
            revision,
            scope: scope.to_owned(),
            forgets,
        })
    }

    pub(crate) fn acknowledge_routed(
        &self,
        target: &str,
        owner: &RoutedOwner,
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
            source_silver: self.silver_certificate()?,
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
