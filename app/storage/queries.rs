use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::generated as sql;
use super::schema::SCHEMA_VERSION;
use super::{Counts, Message, StatusSnapshot, Storage};
use crate::AppError;
use crate::database_error::Operation;

/// A short-lived context reader: share connector ordering, but decode only one
/// window at a time. Construct a new reader for each search.
pub(crate) struct ContextReader<'a> {
    storage: &'a Storage,
    orders: super::connector_views::EntryOrders,
}

impl ContextReader<'_> {
    pub(crate) fn view_content(
        &mut self,
        id: &str,
        context: u32,
        representation: super::MessageContent,
    ) -> Result<Vec<Message>, AppError> {
        if id.starts_with(super::connector_views::ANCHOR_PREFIX) {
            super::connector_views::view_with_orders(
                &self.storage.connection,
                id,
                context,
                &mut self.orders,
            )
        } else {
            self.storage.view_content(id, context, representation)
        }
    }
}

impl Storage {
    /// Canonical history has no dependency on the availability or format of gold.
    /// This reader never attaches or creates a views database, including temp views.
    pub(crate) fn open_silver(path: &Path) -> Result<Self, AppError> {
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
            output_redaction: crate::redaction::Policy::ON,
            connection,
            writer_active: false,
        }
    }

    pub(crate) fn open_existing(path: &Path) -> Result<Self, AppError> {
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
                connectors: Vec::new(),
            });
        }
        if version != SCHEMA_VERSION {
            return Err(super::schema::unsupported(version));
        }
        let storage = Self {
            attachments_config: crate::attachments::Configuration::default(),
            output_redaction: crate::redaction::Policy::ON,
            connection,
            writer_active: false,
        };
        Ok(StatusSnapshot {
            silver: Some(storage.silver_certificate()?),
            counts: storage.silver_counts()?,
            connectors: storage.connector_activity()?,
        })
    }

    /// Every connector source with its declared modes and last deliveries.
    pub(crate) fn connector_activity(&self) -> Result<Vec<crate::source::SourceStatus>, AppError> {
        sql::ConnectorActivity
            .query_many(&self.connection)
            .map_err(AppError::database)?
            .into_iter()
            .map(|row| {
                let modes = row
                    .modes
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|_| AppError::database_data("stored connector modes are invalid"))?
                    .unwrap_or_default();
                Ok(crate::source::SourceStatus {
                    source: crate::source::Source::Connector(row.source_id),
                    modes,
                    last_snapshot_at: row.snapshot_at,
                    last_complete_snapshot_at: row.complete_snapshot_at,
                    last_live_at: row.live_at,
                    last_change_at: row.change_at,
                })
            })
            .collect()
    }

    pub(crate) fn counts(&self) -> Result<Counts, AppError> {
        self.silver_counts()
    }

    /// Native history counts without consulting derived embedding coverage.
    pub(crate) fn silver_counts(&self) -> Result<Counts, AppError> {
        let (conversations, messages, searchable): (i64, i64, i64) = self
            .connection
            .query_row(
                "SELECT
             (SELECT count(*) FROM objects INDEXED BY objects_native_container
              WHERE origin='provider' AND object_role='container'),
             (SELECT count(*) FROM objects INDEXED BY objects_native_id
              WHERE origin='provider' AND object_role<>'container')
             - (SELECT count(*) FROM objects INDEXED BY objects_conversation
                WHERE origin='provider' AND object_role IN ('segment','document')),
             (SELECT count(*) FROM objects INDEXED BY objects_searchable_role
              WHERE origin='provider' AND object_role='entry'
                AND COALESCE(search_projection,content)<>'')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(AppError::database)?;
        let count = |value| {
            u64::try_from(value).map_err(|_| AppError::database_data("negative silver count"))
        };
        Ok(Counts {
            conversations: count(conversations)?,
            messages: count(messages)?,
            searchable_messages: count(searchable)?,
            ..Counts::default()
        })
    }

    pub(crate) fn view(&self, id: &str, context: u32) -> Result<Vec<Message>, AppError> {
        self.view_content(id, context, super::MessageContent::Canonical)
    }

    pub(crate) fn context_reader(&self) -> ContextReader<'_> {
        ContextReader {
            storage: self,
            orders: super::connector_views::EntryOrders::default(),
        }
    }

    pub(crate) fn view_content(
        &self,
        id: &str,
        context: u32,
        representation: super::MessageContent,
    ) -> Result<Vec<Message>, AppError> {
        if id.starts_with(super::connector_views::ANCHOR_PREFIX) {
            return self.connector_view(id, context);
        }
        let anchor = sql::Anchor::builder()
            .id(id)
            .build()
            .query_opt(&self.connection)
            .map_err(AppError::database)?;
        let Some(anchor) = anchor else {
            return Ok(Vec::new());
        };
        let context = i64::from(context);
        let anchor_ordinal = super::required(anchor.ordinal, "ordinal")?;
        if anchor.context_parent.is_some() {
            return self.view_branch(&anchor, anchor_ordinal, context, representation);
        }
        let rows = sql::ViewMessages::builder()
            .conversation_id(&anchor.conversation_id)
            .first_ordinal(Some(anchor_ordinal.saturating_sub(context)))
            .last_ordinal(Some(anchor_ordinal.saturating_add(context)))
            .build()
            .query_many(&self.connection)
            .map_err(AppError::database)?;
        rows.into_iter()
            .map(|row| {
                Ok(Message {
                    attachments: Vec::new(),
                    id: row.id,
                    ordinal: super::required(row.ordinal, "ordinal")?,
                    role: Some(super::required(row.role, "role")?.into()),
                    author_kind: None,
                    content: super::required(
                        match representation {
                            super::MessageContent::Canonical => row.content,
                            super::MessageContent::SearchProjection => row.projection,
                        },
                        "content",
                    )?,
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
        let read = |ordinal| {
            sql::ContextMessage::builder()
                .conversation_id(&anchor.conversation_id)
                .ordinal(Some(ordinal))
                .build()
                .query_opt(&self.connection)
                .map_err(AppError::database)
        };
        let mut rows = Vec::new();
        let mut ordinal = anchor_ordinal;
        for _ in 0..=context {
            let Some(row) = read(ordinal)? else { break };
            let parent = row.context_parent;
            rows.push(row);
            let Some(parent) = parent.filter(|n| *n >= 0 && *n < ordinal) else {
                break;
            };
            ordinal = parent;
        }
        ordinal = anchor_ordinal;
        for _ in 0..context {
            let Some(child) = sql::FirstChild::builder()
                .conversation_id(&anchor.conversation_id)
                .parent(Some(ordinal))
                .build()
                .query_opt(&self.connection)
                .map_err(AppError::database)?
            else {
                break;
            };
            let child = super::required(child.ordinal, "ordinal")?;
            if child <= ordinal {
                return Err(AppError::database_data("invalid message context parent"));
            }
            ordinal = child;
            let Some(row) = read(ordinal)? else { break };
            rows.push(row);
        }
        rows.sort_by_key(|row| row.ordinal);
        rows.into_iter()
            .map(|row| {
                Ok(Message {
                    attachments: Vec::new(),
                    id: row.id,
                    ordinal: super::required(row.ordinal, "ordinal")?,
                    role: Some(super::required(row.role, "role")?.into()),
                    author_kind: None,
                    content: super::required(
                        match representation {
                            super::MessageContent::Canonical => row.content,
                            super::MessageContent::SearchProjection => row.projection,
                        },
                        "content",
                    )?,
                    created_at: row.created_at,
                    tools: crate::tools::Activity::decode(row.metadata.as_deref())?,
                })
            })
            .collect()
    }
}
