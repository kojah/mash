//! Native compressed transcript rows and verified cold archives.
use super::Event;
use crate::AppError;
use crate::silver::sqlite_source::MAX_RECORD_BYTES;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Storage {
    File,
    Sqlite,
}

pub(super) struct Cold {
    rowid: i64,
    generation: String,
    name: String,
    sha256: String,
    storage: Storage,
    bytes: i64,
    raw_bytes: i64,
    count: i64,
    last_seq: i64,
}

pub(super) fn cold(connection: &Connection, session: &str) -> Result<Option<Cold>, AppError> {
    // Earlier upstream schemas have no cold tier. They still own hot transcript rows.
    let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='session_transcript_cold_archives' AND type='table')",[],|row|row.get(0)).map_err(AppError::database)?;
    if !exists {
        return Ok(None);
    }
    connection.query_row("SELECT rowid,generation,archive_name,archive_sha256,storage,archive_bytes,raw_bytes,event_count,last_seq
        FROM session_transcript_cold_archives WHERE session_id=?1",[session],|row| {
        let storage: String = row.get("storage")?;
        let storage = Storage::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(&storage))
            .map_err(|_|rusqlite::Error::InvalidQuery)?;
        Ok(Cold {rowid:row.get("rowid")?,generation:row.get("generation")?,name:row.get("archive_name")?,
            sha256:row.get("archive_sha256")?,storage,bytes:row.get("archive_bytes")?,raw_bytes:row.get("raw_bytes")?,
            count:row.get("event_count")?,last_seq:row.get("last_seq")?})
    }).optional().map_err(AppError::database)
}

pub(crate) fn cold_directory(database: &Path) -> PathBuf {
    let parent = database.parent().unwrap_or(database);
    let artifacts = if parent.file_name().is_some_and(|n| n == "agent") {
        parent.parent().unwrap_or(parent).join("sessions")
    } else {
        parent.to_path_buf()
    };
    artifacts.join("cold")
}

impl Cold {
    pub(super) fn dependency(&self, database: &Path) -> Result<Option<PathBuf>, AppError> {
        let prefix = self
            .name
            .strip_suffix(".jsonl.zst")
            .ok_or_else(|| AppError::configuration("invalid OpenClaw cold archive name"))?;
        if prefix.len() != 64
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(AppError::configuration(
                "invalid OpenClaw cold archive name",
            ));
        }
        Ok(
            matches!(self.storage, Storage::File)
                .then(|| cold_directory(database).join(&self.name)),
        )
    }
    pub(super) fn estimated_bytes(&self) -> Result<u64, AppError> {
        u64::try_from(self.raw_bytes)
            .map_err(|_| AppError::configuration("invalid OpenClaw archive byte count"))
    }
    pub(super) fn events(
        &self,
        connection: &Connection,
        database: &Path,
        session: &str,
    ) -> Result<Vec<Event>, AppError> {
        match self.dependency(database)? {
            Some(path) => self.read(std::fs::File::open(path).map_err(AppError::io)?, session),
            None => self.read(
                connection
                    .blob_open(
                        "main",
                        "session_transcript_cold_archives",
                        "archive_blob",
                        self.rowid,
                        true,
                    )
                    .map_err(AppError::database)?,
                session,
            ),
        }
    }
    fn read<R: Read + Seek>(&self, mut input: R, session: &str) -> Result<Vec<Event>, AppError> {
        self.verify(&mut input)?;
        input.seek(SeekFrom::Start(0)).map_err(AppError::io)?;
        self.decode(input, session)
    }

    fn verify<R: Read>(&self, input: &mut R) -> Result<(), AppError> {
        let mut hash = Sha256::new();
        let mut compressed_bytes = 0_i64;
        let mut buffer = [0; 16 * 1024];
        loop {
            let n = input.read(&mut buffer).map_err(AppError::io)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
            compressed_bytes = compressed_bytes
                .checked_add(
                    i64::try_from(n).map_err(|_| AppError::internal("archive size overflow"))?,
                )
                .ok_or_else(|| AppError::configuration("OpenClaw archive is too large"))?;
            if compressed_bytes > self.bytes {
                return Err(AppError::configuration(
                    "OpenClaw cold archive failed verification",
                ));
            }
        }
        if compressed_bytes != self.bytes || hex::encode(hash.finalize()) != self.sha256 {
            return Err(AppError::configuration(
                "OpenClaw cold archive failed verification",
            ));
        }
        Ok(())
    }

    fn decode<R: Read>(&self, input: R, session: &str) -> Result<Vec<Event>, AppError> {
        let mut decoder = zstd::stream::read::Decoder::new(input).map_err(AppError::io)?;
        decoder.window_log_max(23).map_err(AppError::io)?;
        let mut reader = BufReader::new(decoder);
        let mut line = Vec::new();
        let mut raw_bytes = 0_i64;
        let mut seen_header = false;
        let mut events = Vec::new();
        let mut previous = None;
        loop {
            line.clear();
            let bytes = reader
                .by_ref()
                .take(2 * MAX_RECORD_BYTES.unsigned_abs() + 1025)
                .read_until(b'\n', &mut line)
                .map_err(AppError::io)?;
            if bytes == 0 {
                break;
            }
            if i64::try_from(bytes).unwrap_or(i64::MAX) > 2 * MAX_RECORD_BYTES + 1024 {
                return Err(AppError::configuration(
                    "OpenClaw archive record exceeds the ingestion limit",
                ));
            }
            raw_bytes = raw_bytes
                .checked_add(
                    i64::try_from(bytes)
                        .map_err(|_| AppError::internal("archive size overflow"))?,
                )
                .ok_or_else(|| AppError::configuration("OpenClaw archive is too large"))?;
            if raw_bytes > self.raw_bytes {
                return Err(AppError::configuration(
                    "OpenClaw cold archive byte count mismatch",
                ));
            }
            let record: Record = serde_json::from_slice(&line)
                .map_err(|_| AppError::configuration("invalid OpenClaw cold archive record"))?;
            match record {
                Record::Header {
                    version,
                    session_id,
                    generation,
                } if !seen_header
                    && events.is_empty()
                    && version == 1
                    && session_id == session
                    && generation == self.generation =>
                {
                    seen_header = true;
                }
                Record::Header { .. } => {
                    return Err(AppError::configuration(
                        "OpenClaw cold archive header mismatch",
                    ));
                }
                Record::Event { row } if seen_header => {
                    if previous.is_some_and(|seq| seq >= row.seq) || row.created_at < 0 {
                        return Err(AppError::configuration("invalid OpenClaw cold event order"));
                    }
                    previous = Some(row.seq);
                    let mut event: Event = json(&row.event_json)?;
                    event.observed_at = row.created_at;
                    events.push(event);
                }
                Record::Event { .. } | Record::Other if !seen_header => {
                    return Err(AppError::configuration(
                        "OpenClaw cold archive has no header",
                    ));
                }
                Record::Other => {}
                Record::Event { .. } => {
                    return Err(AppError::configuration("invalid OpenClaw cold event"));
                }
            }
        }
        if !seen_header
            || raw_bytes != self.raw_bytes
            || i64::try_from(events.len()).ok() != Some(self.count)
            || previous != Some(self.last_seq)
        {
            return Err(AppError::configuration(
                "OpenClaw cold archive event count mismatch",
            ));
        }
        Ok(events)
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Record {
    Header {
        version: u32,
        #[serde(rename = "sessionId")]
        session_id: String,
        generation: String,
    },
    Event {
        row: StoredEvent,
    },
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]
struct StoredEvent {
    seq: i64,
    event_json: String,
    created_at: i64,
}

pub(super) fn json<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, AppError> {
    if i64::try_from(input.len()).unwrap_or(i64::MAX) > MAX_RECORD_BYTES {
        return Err(AppError::configuration(
            "OpenClaw event exceeds the 8 MiB ingestion limit",
        ));
    }
    serde_json::from_str(input)
        .map_err(|_| AppError::configuration("invalid OpenClaw transcript event"))
}

pub(super) fn hot(row: &rusqlite::Row<'_>) -> Result<Event, AppError> {
    let bytes: i64 = row.get("bytes").map_err(AppError::database)?;
    if bytes > MAX_RECORD_BYTES {
        return Err(AppError::configuration(
            "OpenClaw event exceeds the 8 MiB ingestion limit",
        ));
    }
    let plain: Option<String> = row.get("event_json").map_err(AppError::database)?;
    let mut event: Event = if let Some(plain) = plain {
        json(&plain)?
    } else {
        let expected: i64 = row.get("event_utf8_bytes").map_err(AppError::database)?;
        if !(1..=4 * 1024 * 1024).contains(&expected) {
            return Err(AppError::configuration(
                "invalid OpenClaw compressed event size",
            ));
        }
        let compressed: Vec<u8> = row.get("event_zstd").map_err(AppError::database)?;
        let mut decoder =
            zstd::stream::read::Decoder::new(compressed.as_slice()).map_err(AppError::io)?;
        decoder.window_log_max(23).map_err(AppError::io)?;
        let mut plain = Vec::new();
        decoder
            .take(expected.unsigned_abs() + 1)
            .read_to_end(&mut plain)
            .map_err(AppError::io)?;
        if i64::try_from(plain.len()).ok() != Some(expected) {
            return Err(AppError::configuration(
                "OpenClaw decompressed event size mismatch",
            ));
        }
        json(
            std::str::from_utf8(&plain)
                .map_err(|_| AppError::configuration("invalid OpenClaw event UTF-8"))?,
        )?
    };
    event.observed_at = row.get("created_at").map_err(AppError::database)?;
    Ok(event)
}
