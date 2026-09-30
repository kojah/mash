//! A bounded read-only snapshot of the native Agent meta/blob tables.
use super::{invalid, models::Metadata};
use crate::{
    AppError,
    ingestion::{records, sqlite_source},
};
use prost::{
    Message,
    encoding::{DecodeContext, WireType, decode_key, decode_varint, skip_field},
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) struct Store {
    pub connection: Connection,
    remaining: usize,
    reads: usize,
}

impl Store {
    pub(super) fn open(path: &Path) -> Result<Self, AppError> {
        let connection = sqlite_source::open(path)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(AppError::database)?;
        if version != 1 {
            return Err(invalid("unsupported native Agent store version"));
        }
        Ok(Self {
            connection,
            remaining: 64 * 1024 * 1024,
            reads: 0,
        })
    }

    pub(super) fn metadata(&self) -> Result<Metadata, AppError> {
        let hex: String = self
            .connection
            .query_row(
                "SELECT CASE WHEN length(value)<=16777216 THEN value END FROM meta WHERE key='0'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| invalid("missing or oversized native metadata"))?;
        let bytes = hex::decode(hex).map_err(|_| invalid("invalid native metadata encoding"))?;
        let metadata: Metadata = records::decode(&bytes)?;
        if metadata.agent_id.trim().is_empty() || metadata.created_at < 0 {
            return Err(invalid("invalid native session identity or timestamp"));
        }
        if let Some(child) = &metadata.subagent_info
            && [
                &child.parent_agent_id,
                &child.root_parent_agent_id,
                &child.tool_call_id,
                &child.type_name,
            ]
            .iter()
            .any(|s| s.trim().is_empty())
        {
            return Err(invalid("invalid native subagent metadata"));
        }
        Ok(metadata)
    }

    pub(super) fn blob(&mut self, id: &[u8]) -> Result<Vec<u8>, AppError> {
        if id.len() != 32 || self.reads == 65_536 {
            return Err(invalid("invalid blob identity or too many reachable blobs"));
        }
        self.reads += 1;
        let key = hex::encode(id);
        let data: Vec<u8> = self
            .connection
            .query_row(
                "SELECT CASE WHEN length(data)<=8388608 THEN data END FROM blobs WHERE id=?1",
                [&key],
                |r| r.get(0),
            )
            .map_err(|_| invalid("missing or oversized reachable blob"))?;
        self.remaining = self
            .remaining
            .checked_sub(data.len())
            .ok_or_else(|| invalid("reachable history exceeds the 64 MiB read budget"))?;
        if Sha256::digest(&data).as_slice() != id {
            return Err(invalid("reachable blob digest mismatch"));
        }
        Ok(data)
    }

    pub(super) fn protobuf<T: Message + Default>(
        &mut self,
        id: &[u8],
        nested: &[u32],
    ) -> Result<T, AppError> {
        let bytes = self.blob(id)?;
        let mut budget = 65_536;
        inspect(&bytes, nested, &mut budget)?;
        T::decode(bytes.as_slice()).map_err(|_| invalid("invalid native protobuf blob"))
    }
}

// The selected models have one nested message level. Inspect those known
// fields before prost allocates repeated containers; unknown fields are skipped.
pub(super) fn inspect(
    mut input: &[u8],
    nested: &[u32],
    remaining: &mut usize,
) -> Result<(), AppError> {
    while !input.is_empty() {
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| invalid("protobuf field limit exceeded"))?;
        let (tag, wire) = decode_key(&mut input).map_err(|_| invalid("invalid protobuf key"))?;
        if wire == WireType::LengthDelimited && nested.contains(&tag) {
            let size = usize::try_from(
                decode_varint(&mut input).map_err(|_| invalid("invalid protobuf length"))?,
            )
            .map_err(|_| invalid("oversized protobuf length"))?;
            let bytes = input
                .get(..size)
                .ok_or_else(|| invalid("truncated protobuf field"))?;
            inspect(bytes, &[], remaining)?;
            input = &input[size..];
        } else {
            skip_field(wire, tag, &mut input, DecodeContext::default())
                .map_err(|_| invalid("invalid protobuf field"))?;
        }
    }
    Ok(())
}
