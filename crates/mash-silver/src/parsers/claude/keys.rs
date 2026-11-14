//! Ordered dialogue UUIDs and first positions with bounded private overflow.
use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};

use crate::AppError;

const INLINE_BYTES: usize = 2 * 1024 * 1024;

pub struct Keys {
    state: State,
    length: usize,
    limit: usize,
}

enum State {
    Inline {
        values: Vec<Option<String>>,
        first: BTreeMap<String, usize>,
        bytes: usize,
    },
    Disk(Store),
    Failed,
}

struct Store {
    connection: Connection,
    _directory: tempfile::TempDir,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            state: State::Inline {
                values: Vec::new(),
                first: BTreeMap::new(),
                bytes: 0,
            },
            length: 0,
            limit: INLINE_BYTES,
        }
    }
}

impl Keys {
    #[cfg(test)]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn push(&mut self, key: Option<String>) -> Result<(), AppError> {
        let result = self.push_inner(key);
        if result.is_err() {
            self.state = State::Failed;
        }
        result
    }

    fn push_inner(&mut self, key: Option<String>) -> Result<(), AppError> {
        let next = self
            .length
            .checked_add(1)
            .ok_or_else(|| AppError::internal("too many Claude dialogue identities"))?;
        match &mut self.state {
            State::Inline {
                values,
                first,
                bytes,
            } => {
                // Include vector growth slack, string capacity and first-position
                // tree allocation. This is retention accounting, not an RSS cap.
                let retained = 2 * std::mem::size_of::<Option<String>>()
                    + key.as_ref().map_or(0, String::capacity)
                    + key
                        .as_ref()
                        .filter(|key| !first.contains_key(*key))
                        .map_or(0, |key| 128_usize.saturating_add(key.len()));
                let total = bytes.saturating_add(retained);
                if total <= self.limit {
                    if let Some(key) = &key {
                        first.entry(key.clone()).or_insert(self.length);
                    }
                    values.push(key);
                    *bytes = total;
                    self.length = next;
                    return Ok(());
                }
            }
            State::Disk(store) => {
                store.insert(self.length, key.as_deref())?;
                self.length = next;
                return Ok(());
            }
            State::Failed => return Err(staging_failed()),
        }
        let old = std::mem::replace(&mut self.state, State::Failed);
        let State::Inline { values, .. } = old else {
            return Err(staging_failed());
        };
        let store = Store::new()?;
        for (position, value) in values.into_iter().enumerate() {
            store.insert(position, value.as_deref())?;
        }
        store.insert(self.length, key.as_deref())?;
        self.state = State::Disk(store);
        self.length = next;
        tracing::info!(
            event = "dialogue_identity_staging_spill",
            entries = self.length,
            inline_budget_bytes = self.limit
        );
        Ok(())
    }

    pub fn get(&self, position: usize) -> Result<Option<String>, AppError> {
        match &self.state {
            State::Inline { values, .. } => values
                .get(position)
                .cloned()
                .ok_or_else(|| AppError::internal("Claude dialogue position is missing")),
            State::Disk(store) => store.get(position),
            State::Failed => Err(staging_failed()),
        }
    }

    pub fn first(&self, key: &str) -> Result<Option<usize>, AppError> {
        match &self.state {
            State::Inline { first, .. } => Ok(first.get(key).copied()),
            State::Disk(store) => store.first(key),
            State::Failed => Err(staging_failed()),
        }
    }
}

fn staging_failed() -> AppError {
    AppError::internal("Claude dialogue identity staging previously failed")
}

impl Store {
    fn new() -> Result<Self, AppError> {
        let directory = crate::scratch::directory("mash-dialogue-identities-")?;
        let connection = crate::scratch::sqlite(&directory.path().join("keys.sqlite3"))?;
        connection
            .execute_batch(
                "PRAGMA cache_size = -1024;
             PRAGMA mmap_size = 0;
             PRAGMA temp_store = FILE;
             PRAGMA journal_mode = OFF;
             PRAGMA synchronous = OFF;
             CREATE TABLE dialogue (position INTEGER PRIMARY KEY, id BLOB);
             CREATE TABLE first_positions (id BLOB PRIMARY KEY NOT NULL,
                 position INTEGER NOT NULL) WITHOUT ROWID;
             BEGIN;",
            )
            .map_err(AppError::database)?;
        Ok(Self {
            connection,
            _directory: directory,
        })
    }

    fn insert(&self, position: usize, key: Option<&str>) -> Result<(), AppError> {
        let position = i64::try_from(position)
            .map_err(|_| AppError::internal("too many Claude dialogue identities"))?;
        self.connection
            .prepare_cached("INSERT INTO dialogue(position,id) VALUES (?1,?2)")
            .map_err(AppError::database)?
            .execute(rusqlite::params![position, key.map(str::as_bytes)])
            .map_err(AppError::database)?;
        if let Some(key) = key {
            self.connection
                .prepare_cached(
                    "INSERT INTO first_positions(id,position) VALUES (?1,?2)
                 ON CONFLICT(id) DO NOTHING",
                )
                .map_err(AppError::database)?
                .execute(rusqlite::params![key.as_bytes(), position])
                .map_err(AppError::database)?;
        }
        Ok(())
    }

    fn get(&self, position: usize) -> Result<Option<String>, AppError> {
        let position = i64::try_from(position)
            .map_err(|_| AppError::internal("Claude dialogue position exceeds SQLite range"))?;
        let bytes: Option<Vec<u8>> = self
            .connection
            .prepare_cached("SELECT id FROM dialogue WHERE position=?1")
            .map_err(AppError::database)?
            .query_row([position], |row| row.get("id"))
            .map_err(AppError::database)?;
        bytes
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| AppError::internal("Claude staged identity is not UTF-8"))
    }

    fn first(&self, key: &str) -> Result<Option<usize>, AppError> {
        let position: Option<i64> = self
            .connection
            .prepare_cached("SELECT position FROM first_positions WHERE id=?1")
            .map_err(AppError::database)?
            .query_row([key.as_bytes()], |row| row.get("position"))
            .optional()
            .map_err(AppError::database)?;
        position
            .map(|position| {
                usize::try_from(position)
                    .map_err(|_| AppError::internal("Claude staged position is invalid"))
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests;
