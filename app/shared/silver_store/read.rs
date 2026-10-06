//! Silver reads: thread metadata, canonical views and retained-history export.
use super::{Identity, database, decode, scope, sql};
use crate::shared::protocol::{ConversationRecord, MessageRecord};
use crate::{AppError, message_context::MessageContext, storage::Message};
use rusqlite::Connection;
use std::io::Write;

/// One published thread's catalogue row.
pub(in crate::shared) struct Thread {
    pub(in crate::shared) node_id: String,
    pub(in crate::shared) conversation: ConversationRecord,
}

pub(in crate::shared) fn threads(
    connection: &Connection,
    group: &str,
    nodes: &[String],
) -> Result<Vec<Thread>, AppError> {
    let mut output = Vec::new();
    for node in nodes {
        for row in sql::NodeConversations::builder()
            .group_id(group)
            .node_id(node)
            .build()
            .query_many(connection)
            .map_err(database)?
        {
            output.push(Thread {
                node_id: node.clone(),
                conversation: decode(&row.record)?,
            });
        }
    }
    Ok(output)
}

struct Row {
    ordinal: i64,
    parent: Option<i64>,
    record: MessageRecord,
}

fn row(ordinal: i64, parent: Option<i64>, record: &str) -> Result<Row, AppError> {
    Ok(Row {
        ordinal,
        parent,
        record: decode(record)?,
    })
}

/// Canonical messages around one anchor: an ordinal window for linear threads,
/// or the native ancestor chain and first-child continuation for trees.
pub(in crate::shared) fn view(
    connection: &Connection,
    identity: &Identity,
    id: &str,
    context: u32,
) -> Result<Vec<MessageRecord>, AppError> {
    let (group, node) = scope(identity);
    let Some(anchor) = sql::Message::builder()
        .group_id(group)
        .node_id(node)
        .id(id)
        .build()
        .query_opt(connection)
        .map_err(database)?
    else {
        return Ok(Vec::new());
    };
    let anchor = row(anchor.ordinal, anchor.parent, &anchor.record)?;
    let thread = anchor.record.conversation_id.clone();
    let context = i64::from(context);
    if anchor.record.context == MessageContext::Linear {
        return sql::MessageWindow::builder()
            .group_id(group)
            .node_id(node)
            .conversation_id(&thread)
            .first_ordinal(anchor.ordinal.saturating_sub(context))
            .last_ordinal(anchor.ordinal.saturating_add(context))
            .build()
            .query_many(connection)
            .map_err(database)?
            .into_iter()
            .map(|row| decode(&row.record))
            .collect();
    }
    let mut rows = vec![anchor];
    let mut ordinal = rows[0].ordinal;
    for _ in 0..context {
        let Some(parent) = rows
            .last()
            .and_then(|row| row.parent)
            .filter(|parent| *parent >= 0 && *parent < ordinal)
        else {
            break;
        };
        let Some(next) = sql::MessageAt::builder()
            .group_id(group)
            .node_id(node)
            .conversation_id(&thread)
            .ordinal(parent)
            .build()
            .query_opt(connection)
            .map_err(database)?
        else {
            break;
        };
        let next = row(next.ordinal, next.parent, &next.record)?;
        ordinal = next.ordinal;
        rows.push(next);
    }
    ordinal = rows[0].ordinal;
    for _ in 0..context {
        let Some(child) = sql::FirstChild::builder()
            .group_id(group)
            .node_id(node)
            .conversation_id(&thread)
            .parent(Some(ordinal))
            .build()
            .query_opt(connection)
            .map_err(database)?
        else {
            break;
        };
        if child.ordinal <= ordinal {
            return Err(AppError::database_data("invalid message context parent"));
        }
        ordinal = child.ordinal;
        rows.push(row(child.ordinal, child.parent, &child.record)?);
    }
    rows.sort_by_key(|row| row.ordinal);
    Ok(rows.into_iter().map(|row| row.record).collect())
}

pub(in crate::shared) fn message(record: MessageRecord, projection: bool) -> Message {
    Message {
        attachments: record.attachments,
        id: record.id,
        ordinal: record.ordinal,
        role: record.role,
        content: if projection {
            record.projection
        } else {
            record.content
        },
        created_at: record.created_at,
    }
}

/// Writes one JSON object member per row without collecting the node in memory.
struct Members<'a, W: Write> {
    output: &'a mut W,
    first: bool,
}

impl<W: Write> Members<'_, W> {
    fn push(&mut self, key: &str, value: &str) -> Result<(), AppError> {
        let io = |_| AppError::internal("write retained shared silver");
        if !std::mem::take(&mut self.first) {
            self.output.write_all(b",").map_err(io)?;
        }
        serde_json::to_writer(&mut *self.output, key)
            .map_err(|_| AppError::internal("encode retained silver key"))?;
        self.output.write_all(b":").map_err(io)?;
        self.output.write_all(value.as_bytes()).map_err(io)
    }
}

fn section<'a, W: Write>(
    output: &'a mut W,
    name: &str,
    first: bool,
) -> Result<Members<'a, W>, AppError> {
    let opening = format!("{}\"{name}\":{{", if first { "{" } else { "}," });
    output
        .write_all(opening.as_bytes())
        .map_err(|_| AppError::internal("write retained shared silver"))?;
    Ok(Members {
        output,
        first: true,
    })
}

/// Stream the retained history wire format: conversations, messages, sources and tombstones.
pub(in crate::shared) fn export(
    connection: &Connection,
    identity: &Identity,
    output: &mut impl Write,
) -> Result<(), AppError> {
    let (group, node) = scope(identity);
    let query = sql::NodeConversations::builder()
        .group_id(group)
        .node_id(node)
        .build();
    let mut statement = query.prepare(connection).map_err(database)?;
    let mut rows = statement.query(query.as_params()).map_err(database)?;
    let mut members = section(output, "conversations", true)?;
    while let Some(row) = rows.next().map_err(database)? {
        let row = sql::NodeConversationsRow::from_row(row).map_err(database)?;
        members.push(&row.id, &row.record)?;
    }
    drop(rows);
    let query = sql::NodeMessages::builder()
        .group_id(group)
        .node_id(node)
        .build();
    let mut statement = query.prepare(connection).map_err(database)?;
    let mut rows = statement.query(query.as_params()).map_err(database)?;
    let mut members = section(output, "messages", false)?;
    while let Some(row) = rows.next().map_err(database)? {
        let row = sql::NodeMessagesRow::from_row(row).map_err(database)?;
        let record: MessageRecord = decode(&row.record)?;
        let value = serde_json::to_string(&record.silver_ref())
            .map_err(|_| AppError::internal("encode retained message"))?;
        members.push(&row.id, &value)?;
    }
    drop(rows);
    let mut members = section(output, "sources", false)?;
    for row in sql::NodeSources::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_many(connection)
        .map_err(database)?
    {
        let source: crate::shared::protocol::Checkpoint = decode(&row.record)?;
        let value = serde_json::to_string(&crate::shared::silver_read::Facts::from(source))
            .map_err(|_| AppError::internal("encode retained source"))?;
        members.push(&row.key, &value)?;
    }
    let mut members = section(output, "tombstones", false)?;
    for row in sql::NodeTombstones::builder()
        .group_id(group)
        .node_id(node)
        .build()
        .query_many(connection)
        .map_err(database)?
    {
        members.push(&row.key, &row.record)?;
    }
    output
        .write_all(b"}}")
        .map_err(|_| AppError::internal("write retained shared silver"))
}
