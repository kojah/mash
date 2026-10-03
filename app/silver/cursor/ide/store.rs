//! Bounded TEXT-only reads from the native application KV store.
use super::{AppError, invalid};
use crate::silver::{records, sqlite_source};
use rusqlite::Connection;
use rusqlite::params;
use serde::de::DeserializeOwned;
use std::path::Path;

pub(super) struct Store {
    pub connection: Connection,
    remaining: usize,
}
impl Store {
    pub(super) fn open(path: &Path) -> Result<Self, AppError> {
        let connection = sqlite_source::open(path)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(AppError::database)?;
        if version != 1 {
            return Err(invalid("unsupported native database version"));
        }
        Ok(Self {
            connection,
            remaining: 64 * 1024 * 1024,
        })
    }

    pub(super) fn json<T: DeserializeOwned>(&mut self, key: &str) -> Result<T, AppError> {
        let remaining =
            i64::try_from(self.remaining).map_err(|_| invalid("invalid read budget"))?;
        let (bytes,value): (i64,Option<String>) = self.connection.query_row(
            "SELECT length(CAST(value AS BLOB)), CASE WHEN typeof(value)='text' AND length(CAST(value AS BLOB))<=8388608 AND length(CAST(value AS BLOB))<=?2 THEN value END FROM cursorDiskKV WHERE key=?1",
            params![key,remaining], |r| Ok((r.get(0)?,r.get(1)?)),
        ).map_err(|_| invalid("missing, oversized or non-TEXT reachable record"))?;
        if bytes > 8 * 1024 * 1024 {
            return Err(invalid("oversized reachable record"));
        }
        if bytes > remaining {
            return Err(invalid("reachable history exceeds the 64 MiB read budget"));
        }
        let value = value.ok_or_else(|| invalid("non-TEXT reachable record"))?;
        self.remaining = self
            .remaining
            .checked_sub(value.len())
            .ok_or_else(|| invalid("reachable history exceeds the 64 MiB read budget"))?;
        records::decode(value.as_bytes())
    }

    pub(super) fn sessions(&self) -> Result<Vec<String>, AppError> {
        let mut statement = self.connection.prepare(
            "SELECT CASE WHEN length(CAST(key AS BLOB))<=1024 THEN key END FROM cursorDiskKV WHERE key>='composerData:' AND key<'composerData;' ORDER BY key LIMIT 65537",
        ).map_err(AppError::database)?;
        let keys = statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(AppError::database)?;
        let mut sessions = Vec::new();
        let mut bytes = 0usize;
        for key in keys {
            let key = key.map_err(|_| invalid("invalid or oversized composer identity"))?;
            let id = key
                .strip_prefix("composerData:")
                .ok_or_else(|| invalid("invalid composer key"))?;
            if id.trim().is_empty() || sessions.len() == 65_536 {
                return Err(invalid("invalid identity or too many composers"));
            }
            bytes = bytes.saturating_add(key.capacity()).saturating_add(32);
            if bytes > 8 * 1024 * 1024 {
                return Err(invalid("composer identities exceed the discovery limit"));
            }
            sessions.push(id.to_owned());
        }
        Ok(sessions)
    }
}
