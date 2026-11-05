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

impl Snapshot<'_> {
    pub(crate) fn delta_base(
        &self,
        instance: Option<&str>,
        revision: i64,
    ) -> Result<Option<i64>, AppError> {
        let floor: i64 = self
            .connection
            .query_row(
                "SELECT floor FROM shared_change_state WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::database)?;
        Ok((instance == Some(self.instance.as_str())
            && revision >= floor
            && revision < self.revision)
            .then_some(revision))
    }

    pub(crate) fn removed_threads(&self, after: i64) -> Result<Vec<String>, AppError> {
        let mut statement = self.connection.prepare("SELECT j.conversation_id FROM shared_changes j WHERE j.revision>?1 AND NOT EXISTS(SELECT 1 FROM objects c WHERE c.origin='provider' AND c.object_role='container' AND c.id=j.conversation_id) ORDER BY j.conversation_id").map_err(AppError::database)?;
        statement
            .query_map([after], |row| row.get(0))
            .map_err(AppError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::database)
    }

    #[cfg(test)]
    pub(crate) fn visit_changed(
        &self,
        kind: BatchKind,
        after: i64,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.visit_selected(kind, false, Some(after), accept)
    }
    /// Bound each network batch without materializing the complete local history.
    #[cfg(test)]
    pub(crate) fn visit(
        &self,
        kind: BatchKind,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.visit_ordered(kind, false, accept)
    }

    #[cfg(test)]
    fn visit_ordered(
        &self,
        kind: BatchKind,
        by_thread: bool,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.visit_selected(kind, by_thread, None, accept)
    }

    pub(crate) fn visit_selected(
        &self,
        kind: BatchKind,
        by_thread: bool,
        after: Option<i64>,
        mut accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let sql = match kind {
            BatchKind::Conversations => {
                "SELECT c.id,c.source,c.title,c.created_at,c.updated_at,c.project,c.repository,e.evidence
                 FROM objects c LEFT JOIN conversation_enrichment e ON e.conversation_id=c.id
                 WHERE c.origin='provider' AND c.object_role='container' ORDER BY c.id"
            }
            BatchKind::Messages if by_thread => {
                "SELECT m.id, m.conversation_id, m.ordinal, m.role, m.content,
                COALESCE(m.search_projection, m.content), m.created_at, NULL, NULL, m.context_parent, m.attachments, m.metadata
                FROM objects m WHERE m.origin='provider' AND m.object_role='entry'
                ORDER BY m.conversation_id, m.ordinal, m.id"
            }
            BatchKind::Messages => {
                "SELECT m.id, m.conversation_id, m.ordinal, m.role, m.content,
                COALESCE(m.search_projection, m.content), m.created_at, NULL, NULL, m.context_parent, m.attachments, m.metadata
                FROM objects m
                WHERE m.origin='provider' AND m.object_role='entry' ORDER BY m.id"
            }
            BatchKind::Tombstones => {
                "SELECT provider, conversation_id FROM tombstones ORDER BY provider, conversation_id"
            }
        };
        let sql = selected_query(sql, kind, after)?;
        let mut statement = self.connection.prepare(&sql).map_err(AppError::database)?;
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

fn selected_query(sql: &str, kind: BatchKind, after: Option<i64>) -> Result<String, AppError> {
    let Some(after) = after else {
        return Ok(sql.to_owned());
    };
    // A delta reads only changed threads. Name their indexes: SQLite would
    // otherwise filter every native row through the low-selectivity origin index.
    let (id, table, indexed) = match kind {
        BatchKind::Conversations => (
            "c.id",
            "FROM objects c",
            "FROM objects c INDEXED BY objects_native_container",
        ),
        BatchKind::Messages => (
            "m.conversation_id",
            "FROM objects m",
            "FROM objects m INDEXED BY objects_conversation",
        ),
        BatchKind::Tombstones => ("conversation_id", "", ""),
    };
    let sql = if table.is_empty() {
        sql.to_owned()
    } else if sql.contains(table) {
        sql.replacen(table, indexed, 1)
    } else {
        return Err(AppError::internal("missing publication source table"));
    };
    let (query, order) = sql
        .split_once(" ORDER BY")
        .ok_or_else(|| AppError::internal("missing publication ordering"))?;
    let join = if query.contains(" WHERE ") {
        "AND"
    } else {
        "WHERE"
    };
    Ok(format!(
        "{query} {join} {id} IN (SELECT conversation_id FROM shared_changes WHERE revision>{after}) ORDER BY{order}"
    ))
}

fn message_record(row: &rusqlite::Row<'_>) -> Result<MessageRecord, AppError> {
    Ok(MessageRecord {
        attachments: serde_json::from_str(
            &row.get::<_, String>("attachments")
                .map_err(AppError::database)?,
        )
        .map_err(|_| AppError::database_data("invalid attachment metadata"))?,
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
        generation: None,
        vector: None,
        tools: crate::tools::Activity::decode(
            row.get::<_, Option<String>>("metadata")
                .map_err(AppError::database)?
                .as_deref(),
        )?,
    })
}

/// What a completed publication may remove from the local change journal.
///
/// During a progressive baseline the journal must survive until the baseline is
/// covered *and* contiguous live coverage has been published; acknowledging a
/// sparse recent batch is not enough.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "baseline producers arrive with the client backfill sender; the gate is enforced and tested now"
)]
pub(crate) enum JournalCoverage {
    /// Ordinary incremental publication: this captured range is complete.
    Incremental { through: i64 },
    /// A baseline is still in progress; record the acknowledgement, keep rows.
    BaselinePending,
    /// Baseline complete and contiguous live coverage reached `through`.
    BaselineComplete { through: i64 },
}

impl JournalCoverage {
    /// Highest revision whose journal rows may be deleted, bounded by what was
    /// actually published.
    fn prunable(self, published: i64) -> Option<i64> {
        match self {
            Self::BaselinePending => None,
            Self::Incremental { through } | Self::BaselineComplete { through } => {
                Some(through.min(published))
            }
        }
    }
}

impl Storage {
    /// New publication policy/destination must advance the revision even if
    /// bronze is unchanged. Persist the target now so failed uploads retry the
    /// same revision; acknowledgement remains pending until the HTTP commit.
    pub(crate) fn prepare_publication_target(&self, target: &str) -> Result<(), AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        transaction.execute("UPDATE shared_change_state SET floor=(SELECT revision+CASE WHEN acknowledged>0 THEN 1 ELSE 0 END FROM shared_publication) WHERE singleton=1 AND EXISTS(SELECT 1 FROM shared_publication WHERE target<>?1)", [target]).map_err(AppError::database)?;
        transaction.execute(
            "UPDATE shared_publication SET target=?1,revision=revision+CASE WHEN acknowledged>0 THEN 1 ELSE 0 END WHERE singleton=1 AND target<>?1",
            [target],
        ).map_err(AppError::database)?;
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
        coverage: JournalCoverage,
    ) -> Result<(), AppError> {
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(AppError::database)?;
        transaction.execute(
            "UPDATE shared_publication SET acknowledged = MAX(acknowledged,?2) WHERE singleton = 1 AND target = ?1 AND revision >= ?2",
            (target, revision),
        ).map_err(AppError::database)?;
        if let Some(prune) = coverage.prunable(revision) {
            transaction.execute("DELETE FROM shared_changes WHERE revision<=?2 AND EXISTS(SELECT 1 FROM shared_publication WHERE target=?1 AND acknowledged>=?2)", (target,prune)).map_err(AppError::database)?;
            transaction.execute("UPDATE shared_change_state SET floor=MAX(floor,?2) WHERE singleton=1 AND EXISTS(SELECT 1 FROM shared_publication WHERE target=?1 AND acknowledged>=?2)", (target,prune)).map_err(AppError::database)?;
        }
        transaction.commit().map_err(AppError::database)
    }

    pub(crate) fn publication_is_pending(
        path: &std::path::Path,
        target: &str,
    ) -> Result<bool, AppError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database)?;
        super::packed::register(&connection).map_err(AppError::database)?;
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
