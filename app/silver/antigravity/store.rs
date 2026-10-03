//! One read-only SQLite snapshot; SQL guards prevent oversized BLOB allocation.
use super::{invalid, proto, wire};
use crate::{AppError, silver::sqlite_source};
use prost::Message;
use rusqlite::Connection;
use std::path::Path;

pub(super) struct Root {
    pub trajectory: String,
    pub cascade: String,
    pub kind: i32,
    pub source: i32,
}
pub(super) struct Store {
    connection: Connection,
    bytes: usize,
    fields: usize,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let connection = sqlite_source::open(path)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(AppError::database)?;
        if version != 0 {
            return Err(invalid("unsupported native database version"));
        }
        let indexed: String = connection
            .query_row(
                "SELECT type FROM pragma_table_info('steps') WHERE name='idx' AND pk=1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| invalid("unsupported native step table"))?;
        if !indexed.eq_ignore_ascii_case("INTEGER") {
            return Err(invalid("unsupported native step primary key"));
        }
        Ok(Self {
            connection,
            bytes: 64 * 1024 * 1024,
            fields: 65_536,
        })
    }
    pub fn root(&self) -> Result<Root, AppError> {
        let mut statement = self.connection.prepare("SELECT CASE WHEN typeof(trajectory_id)='text' AND length(CAST(trajectory_id AS BLOB))<=1024 THEN trajectory_id END AS trajectory, CASE WHEN typeof(cascade_id)='text' AND length(CAST(cascade_id AS BLOB))<=1024 THEN cascade_id END AS cascade, trajectory_type AS kind, source FROM trajectory_meta LIMIT 2").map_err(AppError::database)?;
        let mut rows = statement.query([]).map_err(AppError::database)?;
        let row = rows
            .next()
            .map_err(AppError::database)?
            .ok_or_else(|| invalid("missing trajectory identity"))?;
        let root = Root {
            trajectory: row
                .get("trajectory")
                .map_err(|_| invalid("invalid trajectory identity"))?,
            cascade: row
                .get("cascade")
                .map_err(|_| invalid("invalid cascade identity"))?,
            kind: row.get("kind").map_err(AppError::database)?,
            source: row.get("source").map_err(AppError::database)?,
        };
        if root.trajectory.trim().is_empty()
            || root.cascade.trim().is_empty()
            || rows.next().map_err(AppError::database)?.is_some()
        {
            return Err(invalid("ambiguous or empty trajectory identity"));
        }
        Ok(root)
    }
    pub fn metadata(&mut self) -> Result<proto::Metadata, AppError> {
        let bytes: Option<Vec<u8>> = self.connection.query_row("SELECT CASE WHEN typeof(data)='blob' AND length(data)<=8388608 THEN data END FROM trajectory_metadata_blob WHERE id='main'", [], |r| r.get(0)).map_err(AppError::database)?;
        let bytes = bytes.ok_or_else(|| invalid("missing or oversized trajectory metadata"))?;
        self.bytes = self
            .bytes
            .checked_sub(bytes.len())
            .ok_or_else(|| invalid("history read budget exceeded"))?;
        wire::inspect(&bytes, wire::Shape::Metadata, &mut self.fields)?;
        proto::Metadata::decode(bytes.as_slice())
            .map_err(|_| invalid("invalid trajectory metadata"))
    }
    pub fn steps(
        &mut self,
        mut consume: impl FnMut(i64, proto::Step) -> Result<(), AppError>,
    ) -> Result<(), AppError> {
        let mut statement = self.connection.prepare("SELECT idx,step_type,status,step_format, length(step_payload)+coalesce(length(metadata),0) AS bytes, CASE WHEN typeof(metadata)='blob' AND length(metadata)<=8388608 AND length(step_payload)+length(metadata)<=?1 THEN metadata END AS metadata, CASE WHEN typeof(step_payload)='blob' AND length(step_payload)<=8388608 AND length(step_payload)+coalesce(length(metadata),0)<=?1 THEN step_payload END AS payload FROM steps ORDER BY idx LIMIT 65537").map_err(AppError::database)?;
        // The initial SQL budget guards each allocation; the decreasing Rust
        // budget rejects aggregate overflow before reading the row's BLOBs.
        let mut rows = statement
            .query([i64::try_from(self.bytes).map_err(|_| invalid("invalid read budget"))?])
            .map_err(AppError::database)?;
        let mut previous = None;
        let mut count = 0;
        while let Some(row) = rows.next().map_err(AppError::database)? {
            count += 1;
            if count > 65_536 {
                return Err(invalid("too many native steps"));
            }
            let index: i64 = row.get("idx").map_err(AppError::database)?;
            if index < 0 || previous.is_some_and(|p| index <= p) {
                return Err(invalid("invalid native step index"));
            }
            previous = Some(index);
            let format: i32 = row.get("step_format").map_err(AppError::database)?;
            if format != 0 {
                return Err(invalid("unsupported native step format"));
            }
            let size = usize::try_from(
                row.get::<_, i64>("bytes")
                    .map_err(|_| invalid("invalid step byte length"))?,
            )
            .map_err(|_| invalid("invalid step byte length"))?;
            self.bytes = self
                .bytes
                .checked_sub(size)
                .ok_or_else(|| invalid("history exceeds 64 MiB read budget"))?;
            let payload: Option<Vec<u8>> = row.get("payload").map_err(AppError::database)?;
            let payload =
                payload.ok_or_else(|| invalid("missing, non-BLOB or oversized native step"))?;
            let metadata: Option<Vec<u8>> = row.get("metadata").map_err(AppError::database)?;
            let step = wire::step(&payload, metadata.as_deref(), &mut self.fields)?;
            if step.kind != row.get::<_, i32>("step_type").map_err(AppError::database)?
                || step.status != row.get::<_, i32>("status").map_err(AppError::database)?
            {
                return Err(invalid("SQL and protobuf step headers disagree"));
            }
            consume(index, step)?;
        }
        Ok(())
    }
}
