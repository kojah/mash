use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::generated as sql;
use super::schema::SCHEMA_VERSION;
use super::{ChatWindow, Counts, Message, StatusSnapshot, Storage};

/// An ID passed to `view` that names no message.
pub(crate) enum UnmatchedId {
    Unknown,
    /// A chat that retains no messages.
    EmptyChat,
    /// A chat, with its first message by display position.
    Chat {
        first: String,
    },
}
use crate::AppError;
use crate::database_error::Operation;

impl Storage {
    /// Canonical history has no dependency on the availability or format of gold.
    /// This reader never attaches or creates a views database, including temp views.
    pub fn open_silver(path: &Path) -> Result<Self, AppError> {
        if !path.is_file() {
            return Err(AppError::missing_database(path));
        }
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database_at(Operation::OpenCatalog))?;
        super::packed::register(&connection).map_err(AppError::database)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(AppError::database_at(Operation::InspectCatalog))?;
        if version == 0 {
            return Err(AppError::search_not_ready(
                "index requires an explicit rebuild; run `mash index`",
            ));
        }
        if version != SCHEMA_VERSION {
            return Err(super::schema::unsupported(version));
        }
        Ok(Self::from_silver(connection))
    }

    pub(super) fn from_silver(connection: Connection) -> Self {
        Self {
            attachments_config: crate::attachments::Configuration::default(),
            attachment_verification: None,
            output_redaction: crate::redaction::Policy::ON,
            connection,
            writer_active: false,
        }
    }

    pub fn open_existing(path: &Path) -> Result<Self, AppError> {
        if !path.is_file() {
            return Err(AppError::missing_database(path));
        }
        Self::open(path)
    }

    pub(crate) fn status_snapshot(path: &Path) -> Result<StatusSnapshot, AppError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(AppError::database_at(Operation::OpenCatalog))?;
        super::packed::register(&connection).map_err(AppError::database)?;
        // Counts and certificate must describe the same committed silver snapshot.
        connection
            .execute_batch("BEGIN")
            .map_err(AppError::database_at(Operation::InspectStatus))?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(AppError::database_at(Operation::InspectStatus))?;
        if version == 0 {
            return Ok(StatusSnapshot {
                silver: None,
                counts: Counts::default(),
            });
        }
        if version != SCHEMA_VERSION {
            return Err(super::schema::unsupported(version));
        }
        let storage = Self {
            attachments_config: crate::attachments::Configuration::default(),
            attachment_verification: None,
            output_redaction: crate::redaction::Policy::ON,
            connection,
            writer_active: false,
        };
        Ok(StatusSnapshot {
            silver: Some(storage.silver_certificate()?),
            counts: storage.silver_counts()?,
        })
    }

    pub(crate) fn counts(&self) -> Result<Counts, AppError> {
        self.silver_counts()
    }

    /// Native history counts without consulting derived embedding coverage.
    pub(crate) fn silver_counts(&self) -> Result<Counts, AppError> {
        let (chats, messages, searchable): (i64, i64, i64) = self
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM chats),
                        (SELECT count(*) FROM messages),
                        (SELECT count(*) FROM messages INDEXED BY messages_searchable_role
                         WHERE COALESCE(search_projection,content)<>'')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(AppError::database)?;
        let count = |value| {
            u64::try_from(value).map_err(|_| AppError::database_data("negative silver count"))
        };
        Ok(Counts {
            chats: count(chats)?,
            messages: count(messages)?,
            searchable_messages: count(searchable)?,
            ..Counts::default()
        })
    }

    pub(crate) fn view(&self, id: &str, context: u32) -> Result<Vec<Message>, AppError> {
        self.view_content(id, context, super::MessageContent::Canonical)
    }

    /// The chat around `messages`, a view anchored at `id`; `None` when `id`
    /// matches no message or nothing was returned.
    pub(crate) fn chat_window(
        &self,
        id: &str,
        messages: &[Message],
    ) -> Result<Option<ChatWindow>, AppError> {
        let (Some(first), Some(last)) = (messages.first(), messages.last()) else {
            return Ok(None);
        };
        let Some(anchor) = sql::Anchor::builder()
            .id(id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?
        else {
            return Ok(None);
        };
        let row = sql::ChatWindow::builder()
            .first_ordinal(first.ordinal)
            .last_ordinal(last.ordinal)
            .chat_id(&anchor.chat_id)
            .build()
            .query_one(&self.connection)
            .map_err(AppError::database)?;
        let count = |value| {
            u64::try_from(value).map_err(|_| AppError::database_data("negative message count"))
        };
        Ok(Some(ChatWindow {
            chat_id: anchor.chat_id,
            message_count: count(row.message_count)?,
            omitted_before: count(row.omitted_before)?,
            omitted_after: count(row.omitted_after)?,
        }))
    }

    /// What an ID that matches no message names instead.
    pub(crate) fn unmatched_id(&self, id: &str) -> Result<UnmatchedId, AppError> {
        let row = sql::ChatFirstMessage::builder()
            .id(id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?;
        Ok(match row {
            None => UnmatchedId::Unknown,
            Some(row) => row
                .first_message_id
                .map_or(UnmatchedId::EmptyChat, |first| UnmatchedId::Chat { first }),
        })
    }

    /// One chat's messages by display position, `offset..offset + limit`.
    pub(crate) fn chat_messages(
        &self,
        chat_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Message>, AppError> {
        let rows = sql::ViewMessages::builder()
            .chat_id(chat_id)
            .first_ordinal(offset)
            .last_ordinal(offset.saturating_add(limit).saturating_sub(1))
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?;
        rows.into_iter()
            .map(|row| {
                Ok(Message {
                    attachments: Vec::new(),
                    id: row.id,
                    ordinal: row.ordinal,
                    role: row.role.into(),
                    content: row.content,
                    created_at: row.created_at,
                    tools: crate::tools::Activity::decode(row.metadata.as_deref())?,
                })
            })
            .collect()
    }

    pub(crate) fn view_content(
        &self,
        id: &str,
        context: u32,
        representation: super::MessageContent,
    ) -> Result<Vec<Message>, AppError> {
        let anchor = sql::Anchor::builder()
            .id(id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?;
        let Some(anchor) = anchor else {
            return Ok(Vec::new());
        };
        let context = i64::from(context);
        let anchor_ordinal = anchor.ordinal;
        if anchor.context_parent.is_some() {
            return self.view_branch(&anchor, anchor_ordinal, context, representation);
        }
        let rows = sql::ViewMessages::builder()
            .chat_id(&anchor.chat_id)
            .first_ordinal(anchor_ordinal.saturating_sub(context))
            .last_ordinal(anchor_ordinal.saturating_add(context))
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?;
        rows.into_iter()
            .map(|row| {
                Ok(Message {
                    attachments: Vec::new(),
                    id: row.id,
                    ordinal: row.ordinal,
                    role: row.role.into(),
                    content: match representation {
                        super::MessageContent::Canonical => row.content,
                        super::MessageContent::SearchProjection => row.projection,
                    },
                    created_at: row.created_at,
                    tools: crate::tools::Activity::decode(row.metadata.as_deref())?,
                })
            })
            .collect()
    }

    fn view_branch(
        &self,
        anchor: &sql::AnchorRow,
        anchor_ordinal: i64,
        context: i64,
        representation: super::MessageContent,
    ) -> Result<Vec<Message>, AppError> {
        let read = |id: &str| {
            sql::ContextMessage::builder()
                .chat_id(&anchor.chat_id)
                .id(id)
                .build()
                .query_opt(&self.connection)
                .map_err(AppError::database)
        };
        let mut rows = Vec::new();
        let mut id = anchor.id.clone();
        let mut previous_ordinal = None;
        for _ in 0..=context {
            let Some(row) = read(&id)? else { break };
            let ordinal = row.ordinal;
            if previous_ordinal.is_some_and(|previous| ordinal >= previous) {
                return Err(AppError::database_data("invalid message context parent"));
            }
            previous_ordinal = Some(ordinal);
            let parent = row.context_parent.clone();
            rows.push(row);
            let Some(parent) = parent.filter(|id| !id.is_empty()) else {
                break;
            };
            id = parent;
        }
        let mut ordinal = anchor_ordinal;
        id.clone_from(&anchor.id);
        for _ in 0..context {
            let Some(child) = sql::FirstChild::builder()
                .chat_id(&anchor.chat_id)
                .parent(Some(&id))
                .build()
                .query_opt(&self.connection)
                .map_err(AppError::database)?
            else {
                break;
            };
            let next_ordinal = child.ordinal;
            if next_ordinal <= ordinal {
                return Err(AppError::database_data("invalid message context parent"));
            }
            ordinal = next_ordinal;
            id = child.id;
            let Some(row) = read(&id)? else { break };
            rows.push(row);
        }
        rows.sort_by_key(|row| row.ordinal);
        rows.into_iter()
            .map(|row| {
                Ok(Message {
                    attachments: Vec::new(),
                    id: row.id,
                    ordinal: row.ordinal,
                    role: row.role.into(),
                    content: match representation {
                        super::MessageContent::Canonical => row.content,
                        super::MessageContent::SearchProjection => row.projection,
                    },
                    created_at: row.created_at,
                    tools: crate::tools::Activity::decode(row.metadata.as_deref())?,
                })
            })
            .collect()
    }
}
