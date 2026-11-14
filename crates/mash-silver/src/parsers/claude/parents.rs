//! Exact native ancestry with bounded inline retention and private disk overflow.
use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};

use super::{Ancestry, ParentLink};
use crate::AppError;

const INLINE_BYTES: usize = 2 * 1024 * 1024;
// Strings, tree nodes and allocator slack; this is accounting, not an RSS cap.
const ENTRY_BYTES: usize = 128;

pub struct Parents {
    state: State,
    limit: usize,
}

enum State {
    Inline {
        values: BTreeMap<String, Ancestry>,
        bytes: usize,
    },
    Disk(Store),
    Failed,
}

struct Store {
    // Close SQLite before removing the sensitive scratch directory.
    connection: Connection,
    _directory: tempfile::TempDir,
}

impl Default for Parents {
    fn default() -> Self {
        Self {
            state: State::Inline {
                values: BTreeMap::new(),
                bytes: 0,
            },
            limit: INLINE_BYTES,
        }
    }
}

fn retained(key: &str, value: &Ancestry) -> usize {
    let parent = match value {
        Ancestry::Known(ParentLink::Parent(parent)) => parent.len(),
        _ => 0,
    };
    ENTRY_BYTES.saturating_add(key.len()).saturating_add(parent)
}

impl Parents {
    pub fn clear(&mut self) {
        self.state = State::Inline {
            values: BTreeMap::new(),
            bytes: 0,
        };
    }

    #[cfg(test)]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// Visit one entry at a time; disk overflow must not be collected back into RAM.
    pub fn visit(
        &self,
        mut visitor: impl FnMut(&str, Ancestry) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        match &self.state {
            State::Inline { values, .. } => {
                for (key, value) in values {
                    visitor(key, value.clone())?;
                }
                Ok(())
            }
            State::Disk(store) => store.visit(visitor),
            State::Failed => Err(AppError::internal("ancestry staging previously failed")),
        }
    }

    pub fn get(&self, key: &str) -> Result<Option<Ancestry>, AppError> {
        match &self.state {
            State::Inline { values, .. } => Ok(values.get(key).cloned()),
            State::Disk(store) => store.get(key),
            State::Failed => Err(AppError::internal("ancestry staging previously failed")),
        }
    }

    pub fn insert(&mut self, key: &str, value: Ancestry) -> Result<(), AppError> {
        let result = self.insert_inner(key, value);
        if result.is_err() {
            self.state = State::Failed;
        }
        result
    }

    fn insert_inner(&mut self, key: &str, value: Ancestry) -> Result<(), AppError> {
        match &mut self.state {
            State::Inline { values, bytes } => {
                let old = values
                    .get(key)
                    .map_or(0, |previous| retained(key, previous));
                let next = bytes
                    .saturating_sub(old)
                    .saturating_add(retained(key, &value));
                if next <= self.limit {
                    values.insert(key.to_owned(), value);
                    *bytes = next;
                    return Ok(());
                }
            }
            State::Disk(store) => return store.insert(key, &value),
            State::Failed => return Err(AppError::internal("ancestry staging previously failed")),
        }
        let old = std::mem::replace(&mut self.state, State::Failed);
        let State::Inline { values, .. } = old else {
            return Err(AppError::internal("invalid ancestry staging state"));
        };
        let entries = values.len();
        let store = Store::new()?;
        for (key, value) in values {
            store.insert(&key, &value)?;
        }
        store.insert(key, &value)?;
        tracing::info!(
            event = "ancestry_staging_spill",
            entries,
            inline_budget_bytes = self.limit
        );
        self.state = State::Disk(store);
        Ok(())
    }
}

impl Store {
    fn visit(
        &self,
        mut visitor: impl FnMut(&str, Ancestry) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT id,state,parent FROM parents ORDER BY id")
            .map_err(AppError::database)?;
        let mut rows = statement.query([]).map_err(AppError::database)?;
        while let Some(row) = rows.next().map_err(AppError::database)? {
            let key: Vec<u8> = row.get("id").map_err(AppError::database)?;
            let key = String::from_utf8(key).map_err(|error| {
                AppError::database(rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(error),
                ))
            })?;
            visitor(&key, decode(row).map_err(AppError::database)?)?;
        }
        Ok(())
    }

    fn new() -> Result<Self, AppError> {
        let directory = crate::scratch::directory("mash-ancestry-")?;
        let connection = crate::scratch::sqlite(&directory.path().join("parents.sqlite3"))?;
        // Disposable parsing state, never a recoverable index. This store is
        // never published; any failure aborts the source instead of recovering it.
        connection
            .execute_batch(
                "PRAGMA cache_size = -1024;
             PRAGMA mmap_size = 0;
             PRAGMA temp_store = FILE;
             PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             CREATE TABLE parents (
                 id BLOB PRIMARY KEY NOT NULL,
                 state INTEGER NOT NULL CHECK (state BETWEEN 0 AND 3),
                 parent BLOB,
                 CHECK ((state = 2 AND parent IS NOT NULL) OR (state != 2 AND parent IS NULL))
             ) WITHOUT ROWID;
             BEGIN;",
            )
            .map_err(AppError::database)?;
        Ok(Self {
            connection,
            _directory: directory,
        })
    }

    fn insert(&self, key: &str, value: &Ancestry) -> Result<(), AppError> {
        let (state, parent): (i64, Option<&[u8]>) = match value {
            Ancestry::Known(ParentLink::Missing) => (0, None),
            Ancestry::Known(ParentLink::Root) => (1, None),
            Ancestry::Known(ParentLink::Parent(parent)) => (2, Some(parent.as_bytes())),
            Ancestry::Ambiguous => (3, None),
        };
        self.connection
            .prepare_cached(
                "INSERT INTO parents(id,state,parent) VALUES (?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET state=excluded.state,parent=excluded.parent",
            )
            .map_err(AppError::database)?
            .execute(rusqlite::params![key.as_bytes(), state, parent])
            .map(|_| ())
            .map_err(AppError::database)
    }

    fn get(&self, key: &str) -> Result<Option<Ancestry>, AppError> {
        self.connection
            .prepare_cached("SELECT state,parent FROM parents WHERE id=?1")
            .map_err(AppError::database)?
            .query_row([key.as_bytes()], decode)
            .optional()
            .map_err(AppError::database)
    }
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Ancestry> {
    let state: i64 = row.get("state")?;
    let parent: Option<Vec<u8>> = row.get("parent")?;
    match (state, parent) {
        (0, None) => Ok(Ancestry::Known(ParentLink::Missing)),
        (1, None) => Ok(Ancestry::Known(ParentLink::Root)),
        (2, Some(parent)) => String::from_utf8(parent)
            .map(|parent| Ancestry::Known(ParentLink::Parent(parent)))
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Blob,
                    Box::new(error),
                )
            }),
        (3, None) => Ok(Ancestry::Ambiguous),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

#[cfg(test)]
mod tests;
