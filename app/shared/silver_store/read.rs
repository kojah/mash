//! Silver reads: thread metadata, canonical views and retained-history export.
use super::{conversations, database, message_record, sql};
#[cfg(test)]
use super::{encode, tombstones};
use crate::shared::protocol::{ConversationRecord, MessageRecord};
use crate::{AppError, message_context::MessageContext, storage::Message};
use rusqlite::Connection;
#[cfg(test)]
use std::io::Write;

/// One published thread's catalogue row.
pub(crate) struct Chat {
    pub(crate) node_id: String,
    pub(crate) conversation: ConversationRecord,
}

/// Every thread of one node's silver.
pub(crate) fn threads(connection: &Connection, node: &str) -> Result<Vec<Chat>, AppError> {
    Ok(conversations(connection)?
        .into_iter()
        .map(|conversation| Chat {
            node_id: node.to_owned(),
            conversation,
        })
        .collect())
}

/// A message with the native tree parent, if any.
fn parent(record: &MessageRecord) -> Option<i64> {
    match record.context {
        MessageContext::Parent { ordinal } => Some(ordinal),
        MessageContext::Linear | MessageContext::Root => None,
    }
}

/// Canonical messages around one anchor: an ordinal window for linear threads,
/// or the native ancestor chain and first-child continuation for trees.
pub(crate) fn view(
    connection: &Connection,
    id: &str,
    context: u32,
) -> Result<Vec<MessageRecord>, AppError> {
    let Some(anchor) = sql::Message::builder()
        .id(id)
        .build()
        .query_opt(connection)
        .map_err(database)?
    else {
        return Ok(Vec::new());
    };
    let anchor = message_record!(anchor)?;
    let thread = anchor.conversation_id.clone();
    let context = i64::from(context);
    if anchor.context == MessageContext::Linear {
        return sql::MessageWindow::builder()
            .conversation_id(&thread)
            .first_ordinal(Some(anchor.ordinal.saturating_sub(context)))
            .last_ordinal(Some(anchor.ordinal.saturating_add(context)))
            .build()
            .query_many(connection)
            .map_err(database)?
            .into_iter()
            .map(|row| message_record!(row))
            .collect();
    }
    let mut rows = vec![anchor];
    let mut ordinal = rows[0].ordinal;
    for _ in 0..context {
        let Some(parent) = rows
            .last()
            .and_then(parent)
            .filter(|parent| *parent < ordinal)
        else {
            break;
        };
        let Some(next) = sql::MessageAt::builder()
            .conversation_id(&thread)
            .ordinal(Some(parent))
            .build()
            .query_opt(connection)
            .map_err(database)?
        else {
            break;
        };
        let next = message_record!(next)?;
        ordinal = next.ordinal;
        rows.push(next);
    }
    ordinal = rows[0].ordinal;
    for _ in 0..context {
        let Some(child) = sql::FirstChild::builder()
            .conversation_id(&thread)
            .parent(Some(ordinal))
            .build()
            .query_opt(connection)
            .map_err(database)?
        else {
            break;
        };
        let child = message_record!(child)?;
        if child.ordinal <= ordinal {
            return Err(AppError::database_data("invalid message context parent"));
        }
        ordinal = child.ordinal;
        rows.push(child);
    }
    rows.sort_by_key(|row| row.ordinal);
    Ok(rows)
}

pub(crate) fn message(record: MessageRecord, projection: bool) -> Message {
    Message {
        attachments: record.attachments,
        id: record.id,
        ordinal: record.ordinal,
        role: Some(record.role),
        author_kind: None,
        content: if projection {
            record.projection
        } else {
            record.content
        },
        created_at: record.created_at,
        tools: record.tools,
    }
}

#[cfg(test)]
/// Writes one JSON object member per row without collecting the node in memory.
struct Members<'a, W: Write> {
    output: &'a mut W,
    first: bool,
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
/// Stream the retained history wire format: conversations, messages and tombstones.
pub(crate) fn export(connection: &Connection, output: &mut impl Write) -> Result<(), AppError> {
    let mut members = section(output, "conversations", true)?;
    for record in conversations(connection)? {
        members.push(&record.id, &encode(&record)?)?;
    }
    let query = sql::NodeMessages;
    let mut statement = query.prepare(connection).map_err(database)?;
    let mut rows = statement.query(query.as_params()).map_err(database)?;
    let mut members = section(output, "messages", false)?;
    while let Some(row) = rows.next().map_err(database)? {
        let record = message_record!(sql::NodeMessagesRow::from_row(row).map_err(database)?)?;
        let value = serde_json::to_string(&record.silver_ref())
            .map_err(|_| AppError::internal("encode retained message"))?;
        members.push(&record.id, &value)?;
    }
    drop(rows);
    let mut members = section(output, "tombstones", false)?;
    for tombstone in tombstones(connection)? {
        let key = super::key(tombstone.provider, &tombstone.conversation_id);
        members.push(&key, &encode(&tombstone)?)?;
    }
    output
        .write_all(b"}}")
        .map_err(|_| AppError::internal("write retained shared silver"))
}
