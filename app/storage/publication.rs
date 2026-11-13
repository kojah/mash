use crate::provider::Provider;
use crate::shared::protocol::{Amendment, Batch, BatchKind, ChatRecord, MessageRecord, Tombstone};
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
    pub(crate) fn visit_records(
        &self,
        after: Option<i64>,
        policy: crate::redaction::Policy,
        mut accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let mut records = Vec::new();
        let mut bytes = 0;
        super::records::transfer::visit_shared(self.connection, after, policy, |row| {
            bytes += serde_json::to_vec(&row)
                .map_err(|_| AppError::internal("serialize recall row"))?
                .len();
            records.push(row);
            if records.len() >= 128 || bytes >= 1024 * 1024 {
                accept(Batch::Records {
                    records: std::mem::take(&mut records),
                })?;
                bytes = 0;
            }
            Ok(())
        })?;
        if !records.is_empty() {
            accept(Batch::Records { records })?;
        }
        Ok(())
    }
    pub(crate) fn visit_search_messages(
        &self,
        accept: impl FnMut(crate::views::documents::SearchMessage) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        super::search_input::visit(self.connection, None, accept)
    }
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

    pub(crate) fn removed_chats(&self, after: i64) -> Result<Vec<String>, AppError> {
        let mut statement = self.connection.prepare("SELECT j.chat_id FROM shared_changes j WHERE j.revision>?1 AND NOT EXISTS(SELECT 1 FROM chats c WHERE c.id=j.chat_id) ORDER BY j.chat_id").map_err(AppError::database)?;
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
        self.visit_selected(
            kind,
            false,
            Some(Delta {
                after,
                amend: false,
            }),
            accept,
        )
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
        by_chat: bool,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        self.visit_selected(kind, by_chat, None, accept)
    }

    /// Complete chats: every chat, or a delta's changed chats it does not amend.
    pub(crate) fn visit_selected(
        &self,
        kind: BatchKind,
        by_chat: bool,
        delta: Option<Delta>,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let sql = match kind {
            BatchKind::Chats => format!("{CHATS} ORDER BY c.id"),
            BatchKind::Messages if by_chat => {
                format!("{MESSAGES} FROM messages m ORDER BY m.chat_id, m.ordinal, m.id")
            }
            BatchKind::Messages => format!("{MESSAGES} FROM messages m ORDER BY m.id"),
            BatchKind::Tombstones => {
                "SELECT provider, chat_id FROM tombstones ORDER BY provider, chat_id".to_owned()
            }
        };
        let sql = selected_query(&sql, kind, delta)?;
        self.visit_query(kind, &sql, accept)
    }

    /// Changed and new messages of the chats a delta amends, in chat order.
    pub(crate) fn visit_amended_messages(
        &self,
        after: i64,
        accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let sql = format!(
            "{MESSAGES} FROM shared_message_changes j
             JOIN messages m ON m.id=j.message_id AND m.chat_id=j.chat_id
             WHERE j.revision>{after} AND j.chat_id IN ({})
             ORDER BY m.chat_id, m.ordinal, m.id",
            amended_chats(after)
        );
        self.visit_query(BatchKind::Messages, &sql, accept)
    }

    /// Chats a delta amends: the chat record, every message ID it changed or
    /// removed since `after`, and how many messages it holds now.
    pub(crate) fn visit_amendments(
        &self,
        after: i64,
        mut accept: impl FnMut(Amendment) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let mut chats = self
            .connection
            .prepare(&format!(
                "{CHATS} WHERE c.id IN ({}) ORDER BY c.id",
                amended_chats(after)
            ))
            .map_err(AppError::database)?;
        let mut replaced = self
            .connection
            .prepare(
                "SELECT message_id FROM shared_message_changes
                 WHERE chat_id=?1 AND revision>?2 ORDER BY message_id",
            )
            .map_err(AppError::database)?;
        let mut count = self
            .connection
            .prepare("SELECT count(*) FROM messages INDEXED BY messages_chat WHERE chat_id=?1")
            .map_err(AppError::database)?;
        let mut rows = chats.query([]).map_err(AppError::database)?;
        while let Some(row) = rows.next().map_err(AppError::database)? {
            let chat = chat_record(row)?;
            let replaced = replaced
                .query_map(rusqlite::params![chat.id, after], |row| row.get(0))
                .map_err(AppError::database)?
                .collect::<Result<Vec<String>, _>>()
                .map_err(AppError::database)?;
            let messages = count
                .query_row([&chat.id], |row| row.get::<_, i64>(0))
                .map_err(AppError::database)?;
            let messages = u64::try_from(messages)
                .map_err(|_| AppError::database_data("invalid chat message count"))?;
            accept(Amendment {
                chat,
                replaced,
                messages,
            })?;
        }
        Ok(())
    }

    fn visit_query(
        &self,
        kind: BatchKind,
        sql: &str,
        mut accept: impl FnMut(Batch) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let mut statement = self.connection.prepare(sql).map_err(AppError::database)?;
        let mut rows = statement.query([]).map_err(AppError::database)?;
        let mut batch = match kind {
            BatchKind::Chats => Batch::Chats {
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
                Batch::Records { .. } => {
                    return Err(AppError::internal("recall rows require the record visitor"));
                }
                Batch::Chats { records } => {
                    let record = chat_record(row)?;
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
                        chat_id: row.get::<_, String>(1).map_err(AppError::database)?,
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

/// A delta's base revision, and whether it may amend chats instead of
/// resending them whole.
#[derive(Clone, Copy)]
pub(crate) struct Delta {
    pub(crate) after: i64,
    pub(crate) amend: bool,
}

const CHATS: &str =
    "SELECT c.id,c.provider,c.title,c.created_at,c.updated_at,c.project,c.repository,e.evidence
                 FROM chats c LEFT JOIN chat_enrichment e ON e.chat_id=c.id";
const MESSAGES: &str = "SELECT m.id, m.chat_id, m.ordinal, m.role, m.content,
                COALESCE(m.search_projection, m.content), m.created_at, m.context_parent, m.attachments, m.metadata";

/// Chats changed after `after` that a delta amends: still present, journaled
/// message by message, and changed in at most the protocol's limit.
fn amended_chats(after: i64) -> String {
    format!(
        "SELECT c.chat_id FROM shared_changes c
         WHERE c.revision>{after} AND c.whole=0
           AND EXISTS(SELECT 1 FROM chats x WHERE x.id=c.chat_id)
           AND (SELECT count(*) FROM shared_message_changes j
                WHERE j.chat_id=c.chat_id AND j.revision>{after})<={}",
        crate::shared::protocol::MAX_AMENDED_MESSAGES
    )
}

fn selected_query(sql: &str, kind: BatchKind, delta: Option<Delta>) -> Result<String, AppError> {
    let Some(Delta { after, amend }) = delta else {
        return Ok(sql.to_owned());
    };
    // A delta reads only changed chats. Keep message selection on its
    // chat index rather than scanning canonical bodies.
    let (id, table, indexed) = match kind {
        BatchKind::Chats => ("c.id", "FROM chats c", "FROM chats c"),
        BatchKind::Messages => (
            "m.chat_id",
            "FROM messages m",
            "FROM messages m INDEXED BY messages_chat",
        ),
        BatchKind::Tombstones => ("chat_id", "", ""),
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
    let amended = if amend {
        format!(" AND chat_id NOT IN ({})", amended_chats(after))
    } else {
        String::new()
    };
    Ok(format!(
        "{query} {join} {id} IN (SELECT chat_id FROM shared_changes WHERE revision>{after}{amended}) ORDER BY{order}"
    ))
}

fn chat_record(row: &rusqlite::Row<'_>) -> Result<ChatRecord, AppError> {
    Ok(ChatRecord {
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
    })
}

fn message_record(row: &rusqlite::Row<'_>) -> Result<MessageRecord, AppError> {
    Ok(MessageRecord {
        attachments: serde_json::from_str(
            &row.get::<_, String>("attachments")
                .map_err(AppError::database)?,
        )
        .map_err(|_| AppError::database_data("invalid attachment metadata"))?,
        id: row.get::<_, String>(0).map_err(AppError::database)?,
        chat_id: row.get::<_, String>(1).map_err(AppError::database)?,
        ordinal: row.get::<_, i64>(2).map_err(AppError::database)?,
        context: crate::message_context::MessageContext::from_parent(
            row.get("context_parent").map_err(AppError::database)?,
        ),
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
            transaction.execute("DELETE FROM shared_message_changes WHERE revision<=?2 AND EXISTS(SELECT 1 FROM shared_publication WHERE target=?1 AND acknowledged>=?2)", (target,prune)).map_err(AppError::database)?;
            transaction.execute("DELETE FROM record_changes WHERE revision<=?2 AND EXISTS(SELECT 1 FROM shared_publication WHERE target=?1 AND acknowledged>=?2)", (target,prune)).map_err(AppError::database)?;
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
