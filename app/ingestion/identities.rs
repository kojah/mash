//! Exact native identity validation with bounded inline retention.
use crate::AppError;
use rusqlite::Connection;
use std::collections::HashSet;

const INLINE_BYTES: usize = 2 * 1024 * 1024;
// Conservative allowance for String, hash-table slack and allocator overhead.
const ENTRY_BYTES: usize = 96;

pub(super) struct Identities {
    state: State,
    limit: usize,
}

enum State {
    Inline {
        values: HashSet<String>,
        bytes: usize,
    },
    Disk(Store),
    Failed,
}

struct Store {
    // Close SQLite before deleting its private scratch directory.
    connection: Connection,
    _directory: tempfile::TempDir,
}

impl Default for Identities {
    fn default() -> Self {
        Self {
            state: State::Inline {
                values: HashSet::new(),
                bytes: 0,
            },
            limit: INLINE_BYTES,
        }
    }
}

impl Identities {
    pub(super) fn insert(&mut self, value: &str) -> Result<bool, AppError> {
        let result = self.insert_inner(value);
        if result.is_err() {
            self.state = State::Failed;
        }
        result
    }

    fn insert_inner(&mut self, value: &str) -> Result<bool, AppError> {
        match &mut self.state {
            State::Inline { values, bytes } => {
                if values.contains(value) {
                    return Ok(false);
                }
                let retained = value.len().saturating_add(ENTRY_BYTES);
                if bytes.saturating_add(retained) <= self.limit {
                    values.insert(value.to_owned());
                    *bytes = bytes.saturating_add(retained);
                    return Ok(true);
                }
            }
            State::Disk(store) => return store.insert(value),
            State::Failed => return Err(AppError::internal("identity staging previously failed")),
        }
        let old = std::mem::replace(&mut self.state, State::Failed);
        let State::Inline { values, .. } = old else {
            return Err(AppError::internal("invalid identity staging state"));
        };
        let entries = values.len();
        let store = Store::new()?;
        for id in values {
            store.insert(&id)?;
        }
        let inserted = store.insert(value)?;
        tracing::info!(
            event = "identity_staging_spill",
            entries,
            inline_budget_bytes = self.limit
        );
        self.state = State::Disk(store);
        Ok(inserted)
    }
}

impl Store {
    fn new() -> Result<Self, AppError> {
        let directory = crate::scratch::directory("mash-identities-")?;
        let connection = crate::scratch::sqlite(&directory.path().join("ids.sqlite3"))?;
        connection
            .execute_batch(
                "PRAGMA cache_size = -1024;
             PRAGMA mmap_size = 0;
             PRAGMA temp_store = FILE;
             PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             CREATE TABLE ids (id BLOB PRIMARY KEY NOT NULL) WITHOUT ROWID;
             BEGIN;",
            )
            .map_err(AppError::database)?;
        // This store is disposable validation state, never a recoverable index.
        Ok(Self {
            connection,
            _directory: directory,
        })
    }

    fn insert(&self, value: &str) -> Result<bool, AppError> {
        self.connection
            .prepare_cached("INSERT INTO ids(id) VALUES (?1) ON CONFLICT(id) DO NOTHING")
            .map_err(AppError::database)?
            .execute([value.as_bytes()])
            .map(|changed| changed == 1)
            .map_err(AppError::database)
    }
}

#[cfg(test)]
mod tests;
