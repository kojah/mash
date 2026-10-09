//! Message-anchored dense units; canonical history and lexical rows stay intact.
use super::config::{Build, RetrievalUnit};
use crate::{AppError, message_context::MessageContext, role::Role};
use rusqlite::{OptionalExtension, params};

pub(crate) const PARENT_CHARS: usize = 2048;

pub(crate) fn parent_ordinal(context: MessageContext, ordinal: i64) -> Option<i64> {
    match context {
        MessageContext::Linear => ordinal.checked_sub(1).filter(|value| *value >= 0),
        MessageContext::Root => None,
        MessageContext::Parent { ordinal } => Some(ordinal),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Input<'a> {
    pub(crate) chat: &'a str,
    pub(crate) ordinal: i64,
    pub(crate) context: MessageContext,
    pub(crate) role: &'a Role,
    pub(crate) projection: &'a str,
}

/// Private disk-backed parent lookup, bounded to one chat and 2048 chars/row.
/// The default message recipe does not create a scratch database.
#[derive(Default)]
pub(crate) struct Stream {
    parents: Option<Parents>,
}

struct Parents {
    connection: rusqlite::Connection,
    _directory: tempfile::TempDir,
    chat: String,
    ordinal: i64,
}

impl Parents {
    fn new() -> Result<Self, AppError> {
        let directory = crate::scratch::directory("mash-unit-parents-")?;
        let connection = crate::scratch::sqlite(&directory.path().join("parents.sqlite3"))?;
        // Scratch state is disposable, never used for recovery or publication.
        connection
            .execute_batch(
                "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
             PRAGMA cache_size=-2048; PRAGMA temp_store=FILE;
             CREATE TABLE parents(ordinal INTEGER PRIMARY KEY, projection TEXT NOT NULL);",
            )
            .map_err(AppError::database)?;
        Ok(Self {
            connection,
            _directory: directory,
            chat: String::new(),
            ordinal: -1,
        })
    }

    fn observe(&mut self, input: &Input<'_>, build: &Build) -> Result<Option<String>, AppError> {
        if self.chat != input.chat {
            self.connection
                .execute("DELETE FROM parents", [])
                .map_err(AppError::database)?;
            input.chat.clone_into(&mut self.chat);
            self.ordinal = -1;
        }
        if input.ordinal <= self.ordinal || !input.context.valid(input.ordinal) {
            return Err(AppError::internal(
                "gold unit messages are not in valid ordinal order",
            ));
        }
        self.ordinal = input.ordinal;
        let parent = match parent_ordinal(input.context, input.ordinal) {
            Some(ordinal) => self
                .connection
                .query_row(
                    "SELECT projection FROM parents WHERE ordinal=?1",
                    [ordinal],
                    |row| row.get::<_, String>("projection"),
                )
                .optional()
                .map_err(AppError::database)?,
            None => None,
        };
        if build.includes_dense(input.role, input.projection) {
            let projection: String = input.projection.chars().take(PARENT_CHARS).collect();
            self.connection
                .execute(
                    "INSERT INTO parents(ordinal, projection) VALUES (?1, ?2)",
                    params![input.ordinal, projection],
                )
                .map_err(AppError::database)?;
        }
        Ok(parent)
    }
}

impl Stream {
    /// Call on a blocking worker, for every row in chat/ordinal order.
    pub(crate) fn prepare(
        &mut self,
        build: &Build,
        input: Input<'_>,
        title: Option<&str>,
    ) -> Result<Option<String>, AppError> {
        let parent =
            if build.retrieval_unit == RetrievalUnit::ParentAndMessage && build.dense_enabled() {
                if self.parents.is_none() {
                    self.parents = Some(Parents::new()?);
                }
                self.parents
                    .as_mut()
                    .ok_or_else(|| AppError::internal("missing unit parent lookup"))?
                    .observe(&input, build)?
            } else {
                None
            };
        Ok(build
            .includes_dense(input.role, input.projection)
            .then(|| build.prepare_unit(input.projection.to_owned(), title, parent.as_deref())))
    }
}

#[cfg(test)]
mod tests;
