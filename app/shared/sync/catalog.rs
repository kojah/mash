//! Durable transfer catalog and bounded piece spool.
//!
//! Bytes reach the filesystem before the checkpoint that acknowledges them is
//! committed to SQLite. A restart can therefore resume from the last reported
//! checkpoint, and a retry that reuses a sequence is accepted only when its
//! decoded digest matches the bytes already retained.
use super::wire;
use crate::AppError;
use rusqlite::{Connection, OptionalExtension, params};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BUSY_TIMEOUT: Duration = Duration::from_secs(30);
const SCHEMA_VERSION: i64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Receiving,
    QueuedForApply,
    Applying,
    ViewsPending,
    Published,
    Cancelled,
    Failed,
    Expired,
}

impl State {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Receiving => "receiving",
            Self::QueuedForApply => "queued_for_apply",
            Self::Applying => "applying",
            Self::ViewsPending => "views_pending",
            Self::Published => "published",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "receiving" => Some(Self::Receiving),
            "queued_for_apply" => Some(Self::QueuedForApply),
            "applying" => Some(Self::Applying),
            "views_pending" => Some(Self::ViewsPending),
            "published" => Some(Self::Published),
            "cancelled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_field_names,
    reason = "the operation identity names the durable transfer row"
)]
pub struct Operation {
    pub operation_id: String,
    pub kind: String,
    pub manifest_digest: String,
    pub(crate) decoded_bytes: u64,
    pub record_count: u32,
    pub start_json: String,
    pub state: State,
    pub next_sequence: u64,
    pub decoded_offset: u64,
    pub revision: Option<i64>,
    pub receipt_digest: Option<String>,
}

pub struct Begin<'a> {
    pub operation_id: &'a str,
    pub group_id: &'a str,
    pub node_id: &'a str,
    pub kind: &'a str,
    pub manifest_digest: &'a str,
    pub decoded_bytes: u64,
    pub record_count: u32,
    pub start_json: &'a str,
}

pub enum BeginOutcome {
    New(Operation),
    Existing(Operation),
}

/// One durable progressive-backfill session. The manifest is stored in the
/// member table; `covered` counts members with committed newer evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_field_names,
    reason = "the backfill identity names the durable session row"
)]
pub struct Backfill {
    pub backfill_id: String,
    pub(crate) database_instance: String,
    pub baseline_revision: i64,
    pub(crate) views_fingerprint: String,
    pub(crate) manifest_digest: String,
    pub total_chats: u64,
    pub covered: u64,
    pub state: String,
}

pub struct BackfillBegin<'a> {
    pub backfill_id: &'a str,
    pub group_id: &'a str,
    pub node_id: &'a str,
    pub database_instance: &'a str,
    pub baseline_revision: i64,
    pub views_fingerprint: &'a str,
    pub manifest_digest: &'a str,
    pub total_chats: u64,
}

/// A durable operation left behind by a process that stopped mid-apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recoverable {
    pub operation_id: String,
    pub group_id: String,
    pub node_id: String,
    pub kind: String,
}

pub struct StoredChunk {
    pub sequence: u64,
    pub decoded_offset: u64,
    pub decoded_length: u64,
    pub digest: String,
    pub encoded: Vec<u8>,
    pub compression: i32,
}

pub struct Catalog {
    root: PathBuf,
    connection: Connection,
    /// Total encoded bytes all retained pieces may occupy.
    limit_bytes: u64,
}

/// Spool ceiling for tests and tools that do not configure one.
#[cfg(any(test, feature = "test-support"))]
pub const DEFAULT_SPOOL_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn database(error: rusqlite::Error) -> AppError {
    AppError::database(error)
}

fn conflict(message: &str) -> AppError {
    crate::shared::validation::conflict(message)
}

fn now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis()),
    )
    .unwrap_or(i64::MAX)
}

impl Catalog {
    #[cfg(any(test, feature = "test-support"))]
    pub fn open(root: &Path) -> Result<Self, AppError> {
        Self::open_with_limit(root, DEFAULT_SPOOL_LIMIT)
    }

    /// Open the catalog with a hard ceiling on retained spool bytes.
    pub fn open_with_limit(root: &Path, limit_bytes: u64) -> Result<Self, AppError> {
        std::fs::create_dir_all(root.join("pieces")).map_err(AppError::io)?;
        let path = root.join("catalog.sqlite3");
        let connection = Connection::open(&path).map_err(database)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(database)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(database)?;
        let populated: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table')",
                [],
                |row| row.get(0),
            )
            .map_err(database)?;
        if (populated || version != 0) && version != SCHEMA_VERSION {
            return Err(AppError::schema(format!(
                "unsupported sync catalog schema {version}; expected {SCHEMA_VERSION}"
            )));
        }
        connection
            .pragma_update(None, "journal_mode", "wal")
            .map_err(database)?;
        connection
            .pragma_update(None, "synchronous", "full")
            .map_err(database)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS operations (
                    operation_id TEXT PRIMARY KEY,
                    group_id TEXT NOT NULL,
                    node_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    manifest_digest TEXT NOT NULL,
                    decoded_bytes INTEGER NOT NULL,
                    record_count INTEGER NOT NULL,
                    start_json TEXT NOT NULL,
                    state TEXT NOT NULL,
                    next_sequence INTEGER NOT NULL,
                    decoded_offset INTEGER NOT NULL,
                    revision INTEGER,
                    receipt_digest TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS pieces (
                    operation_id TEXT NOT NULL,
                    sequence INTEGER NOT NULL,
                    decoded_offset INTEGER NOT NULL,
                    decoded_length INTEGER NOT NULL,
                    digest TEXT NOT NULL,
                    encoded_length INTEGER NOT NULL,
                    compression INTEGER NOT NULL DEFAULT 1,
                    PRIMARY KEY (operation_id, sequence)
                );
                CREATE INDEX IF NOT EXISTS operations_owner
                    ON operations(group_id, node_id, manifest_digest);
                CREATE TABLE IF NOT EXISTS backfills (
                    backfill_id TEXT PRIMARY KEY,
                    group_id TEXT NOT NULL,
                    node_id TEXT NOT NULL,
                    database_instance TEXT NOT NULL,
                    baseline_revision INTEGER NOT NULL,
                    views_fingerprint TEXT NOT NULL,
                    manifest_digest TEXT NOT NULL,
                    total_chats INTEGER NOT NULL,
                    state TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS backfill_members (
                    backfill_id TEXT NOT NULL,
                    chat_id TEXT NOT NULL,
                    digest TEXT NOT NULL,
                    covered INTEGER NOT NULL DEFAULT 0,
                    live_revision INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (backfill_id, chat_id)
                );",
            )
            .map_err(database)?;
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(database)?;
        Ok(Self {
            root: root.to_path_buf(),
            connection,
            limit_bytes,
        })
    }

    pub fn begin(&self, begin: &Begin<'_>) -> Result<BeginOutcome, AppError> {
        if begin.operation_id.is_empty() || begin.operation_id.len() > 4096 {
            return Err(AppError::usage("invalid sync operation identity"));
        }
        let existing = self.operation(begin.operation_id)?.is_some();
        if existing {
            let operation = self.select(begin.operation_id, begin.group_id, begin.node_id)?;
            if operation.manifest_digest != begin.manifest_digest
                || operation.decoded_bytes != begin.decoded_bytes
                || operation.kind != begin.kind
            {
                return Err(conflict(
                    "sync operation identity was already used for different bytes",
                ));
            }
            // Same records, different start (revision, certificate): a new
            // publication, never a replay of the earlier receipt.
            if operation.start_json != begin.start_json {
                return Err(conflict(
                    "sync operation identity was already used for different publication metadata",
                ));
            }
            return Ok(BeginOutcome::Existing(operation));
        }
        let timestamp = now();
        self.connection
            .execute(
                "INSERT INTO operations (
                    operation_id, group_id, node_id, kind, manifest_digest,
                    decoded_bytes, record_count, start_json, state,
                    next_sequence, decoded_offset, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'receiving', 0, 0, ?9, ?9)",
                params![
                    begin.operation_id,
                    begin.group_id,
                    begin.node_id,
                    begin.kind,
                    begin.manifest_digest,
                    i64::try_from(begin.decoded_bytes)
                        .map_err(|_| AppError::usage("sync operation is too large"))?,
                    i64::from(begin.record_count),
                    begin.start_json,
                    timestamp
                ],
            )
            .map_err(database)?;
        Ok(BeginOutcome::New(self.select(
            begin.operation_id,
            begin.group_id,
            begin.node_id,
        )?))
    }

    /// Persist one validated piece, then advance the durable checkpoint.
    pub fn append(
        &self,
        operation_id: &str,
        group_id: &str,
        node_id: &str,
        chunk: &StoredChunk,
    ) -> Result<Operation, AppError> {
        let operation = self.select(operation_id, group_id, node_id)?;
        match operation.state {
            State::Receiving => {}
            State::Published => return Err(conflict("sync operation is already published")),
            _ => return Err(conflict("sync operation is not receiving data")),
        }
        if chunk.sequence < operation.next_sequence {
            let existing = self
                .piece(operation_id, chunk.sequence)?
                .ok_or_else(|| AppError::internal("retained transfer checkpoint has no piece"))?;
            if existing.digest != chunk.digest || existing.decoded_offset != chunk.decoded_offset {
                return Err(conflict("conflicting retry for a retained sync piece"));
            }
            return Ok(operation);
        }
        if chunk.sequence != operation.next_sequence {
            return Err(conflict("sync piece sequence is not contiguous"));
        }
        if chunk.decoded_offset != operation.decoded_offset {
            return Err(conflict("sync piece offset is not contiguous"));
        }
        let end = operation
            .decoded_offset
            .checked_add(chunk.decoded_length)
            .ok_or_else(|| AppError::usage("sync piece length overflow"))?;
        if end > operation.decoded_bytes {
            return Err(AppError::usage(
                "sync pieces exceed the declared decoded size",
            ));
        }
        self.write_piece(operation_id, chunk)?;
        let transaction = self.connection.unchecked_transaction().map_err(database)?;
        let retained: i64 = transaction
            .query_row(
                "SELECT COALESCE(SUM(encoded_length), 0) FROM pieces",
                [],
                |row| row.get(0),
            )
            .map_err(database)?;
        let incoming = i64::try_from(chunk.encoded.len())
            .map_err(|_| AppError::usage("sync piece is too large"))?;
        if retained.saturating_add(incoming) > i64::try_from(self.limit_bytes).unwrap_or(i64::MAX) {
            transaction.rollback().map_err(database)?;
            let _ = std::fs::remove_file(self.piece_path(operation_id, chunk.sequence));
            return Err(AppError::shared(
                "sync spool quota exceeded; retry after other transfers finish or expire",
            ));
        }
        transaction
            .execute(
                "INSERT INTO pieces (
                    operation_id, sequence, decoded_offset, decoded_length,
                    digest, encoded_length, compression
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    operation_id,
                    i64::try_from(chunk.sequence)
                        .map_err(|_| AppError::usage("sync sequence is too large"))?,
                    i64::try_from(chunk.decoded_offset)
                        .map_err(|_| AppError::usage("sync offset is too large"))?,
                    i64::try_from(chunk.decoded_length)
                        .map_err(|_| AppError::usage("sync piece is too large"))?,
                    chunk.digest,
                    i64::try_from(chunk.encoded.len())
                        .map_err(|_| AppError::usage("sync piece is too large"))?,
                    chunk.compression,
                ],
            )
            .map_err(database)?;
        let changed = transaction
            .execute(
                "UPDATE operations SET next_sequence = ?2, decoded_offset = ?3, updated_at = ?4
                 WHERE operation_id = ?1 AND state = 'receiving' AND next_sequence = ?5",
                params![
                    operation_id,
                    i64::try_from(chunk.sequence + 1)
                        .map_err(|_| AppError::usage("sync sequence is too large"))?,
                    i64::try_from(end).map_err(|_| AppError::usage("sync offset is too large"))?,
                    now(),
                    i64::try_from(chunk.sequence)
                        .map_err(|_| AppError::usage("sync sequence is too large"))?,
                ],
            )
            .map_err(database)?;
        if changed != 1 {
            // A concurrent expiry or takeover closed this transfer between the
            // state check above and this transaction; fail closed.
            transaction.rollback().map_err(database)?;
            return Err(conflict(
                "sync operation is no longer receiving this transfer",
            ));
        }
        transaction.commit().map_err(database)?;
        self.select(operation_id, group_id, node_id)
    }

    /// Verify the complete receipt and queue it for application.
    pub fn finish(
        &self,
        operation_id: &str,
        group_id: &str,
        node_id: &str,
        manifest_digest: &str,
    ) -> Result<(Operation, Vec<wire::Piece>), AppError> {
        let operation = self.select(operation_id, group_id, node_id)?;
        if operation.manifest_digest != manifest_digest {
            return Err(conflict(
                "sync manifest does not match the received operation",
            ));
        }
        if operation.decoded_offset != operation.decoded_bytes {
            return Err(conflict("sync operation is incomplete"));
        }
        let pieces = self.pieces(operation_id)?;
        if pieces.len() as u64 != operation.next_sequence {
            return Err(AppError::internal("durable sync checkpoint lost pieces"));
        }
        let entries = pieces
            .iter()
            .map(|piece| wire::ManifestPiece {
                sequence: piece.sequence,
                decoded_offset: piece.decoded_offset,
                decoded_length: piece.decoded_length,
                digest: &piece.digest,
            })
            .collect::<Vec<_>>();
        if wire::manifest_digest(operation.decoded_bytes, operation.record_count, &entries)
            != operation.manifest_digest
        {
            return Err(conflict("sync manifest does not match the received pieces"));
        }
        if matches!(operation.state, State::Receiving) {
            self.set_state(
                operation_id,
                State::QueuedForApply,
                operation.revision,
                operation.receipt_digest.as_deref(),
            )?;
        }
        Ok((self.select(operation_id, group_id, node_id)?, pieces))
    }

    /// Operations that reached a durable apply state before the process
    /// stopped. Receiving transfers are not listed: they wait for their client.
    pub fn recoverable(&self) -> Result<Vec<Recoverable>, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT operation_id, group_id, node_id, kind FROM operations
                 WHERE state IN ('queued_for_apply', 'applying', 'views_pending')
                 ORDER BY created_at, operation_id",
            )
            .map_err(database)?;
        let rows = statement
            .query_map([], |row| {
                Ok(Recoverable {
                    operation_id: row.get(0)?,
                    group_id: row.get(1)?,
                    node_id: row.get(2)?,
                    kind: row.get(3)?,
                })
            })
            .map_err(database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database)?;
        Ok(rows)
    }

    /// Retained pieces of a recoverable operation, with ownership checks.
    pub fn recover_pieces(
        &self,
        operation_id: &str,
        group_id: &str,
        node_id: &str,
    ) -> Result<(Operation, Vec<wire::Piece>), AppError> {
        let operation = self.select(operation_id, group_id, node_id)?;
        let pieces = self.pieces(operation_id)?;
        Ok((operation, pieces))
    }

    pub fn select(
        &self,
        operation_id: &str,
        group_id: &str,
        node_id: &str,
    ) -> Result<Operation, AppError> {
        let operation = self
            .operation(operation_id)?
            .ok_or_else(|| conflict("unknown sync operation"))?;
        if operation.0 != group_id || operation.1 != node_id {
            return Err(conflict("sync operation belongs to another database"));
        }
        Ok(operation.2)
    }

    /// Operation lookup used only to distinguish an unknown operation from an
    /// operation owned by another authenticated node.
    fn operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<(String, String, Operation)>, AppError> {
        self.connection
            .query_row(
                "SELECT group_id, node_id, kind, manifest_digest, decoded_bytes,
                        record_count, start_json, state, next_sequence,
                        decoded_offset, revision, receipt_digest
                 FROM operations WHERE operation_id = ?1",
                params![operation_id],
                |row| {
                    let state: String = row.get(7)?;
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        Operation {
                            operation_id: operation_id.to_owned(),
                            kind: row.get(2)?,
                            manifest_digest: row.get(3)?,
                            decoded_bytes: row.get::<_, i64>(4)?.try_into().unwrap_or_default(),
                            record_count: row.get::<_, i64>(5)?.try_into().unwrap_or_default(),
                            start_json: row.get(6)?,
                            state: State::parse(&state).ok_or(rusqlite::Error::InvalidQuery)?,
                            next_sequence: row.get::<_, i64>(8)?.try_into().unwrap_or_default(),
                            decoded_offset: row.get::<_, i64>(9)?.try_into().unwrap_or_default(),
                            revision: row.get(10)?,
                            receipt_digest: row.get(11)?,
                        },
                    ))
                },
            )
            .optional()
            .map_err(database)
    }

    pub fn set_state(
        &self,
        operation_id: &str,
        state: State,
        revision: Option<i64>,
        receipt_digest: Option<&str>,
    ) -> Result<(), AppError> {
        self.connection
            .execute(
                "UPDATE operations SET state = ?2, revision = ?3, receipt_digest = ?4,
                        updated_at = ?5 WHERE operation_id = ?1",
                params![
                    operation_id,
                    state.as_str(),
                    revision,
                    receipt_digest,
                    now()
                ],
            )
            .map_err(database)?;
        Ok(())
    }

    pub fn published(
        &self,
        operation_id: &str,
        revision: i64,
        receipt_digest: &str,
    ) -> Result<(), AppError> {
        self.set_state(
            operation_id,
            State::Published,
            Some(revision),
            Some(receipt_digest),
        )?;
        self.remove_pieces(operation_id)
    }

    /// Mark a non-publication operation complete without claiming a silver
    /// revision. Used for the backfill manifest transfer.
    pub fn completed(&self, operation_id: &str, receipt_digest: &str) -> Result<(), AppError> {
        self.set_state(operation_id, State::Published, None, Some(receipt_digest))?;
        self.remove_pieces(operation_id)
    }

    /// Create or re-open a progressive-backfill session. Identity and manifest
    /// are immutable once the session exists.
    pub fn begin_backfill(&self, input: &BackfillBegin<'_>) -> Result<Backfill, AppError> {
        if input.backfill_id.is_empty() || input.backfill_id.len() > 4096 {
            return Err(AppError::usage("invalid backfill identity"));
        }
        if input.baseline_revision < 0
            || input.database_instance.is_empty()
            || input.views_fingerprint.is_empty()
            || input.manifest_digest.len() != 64
            || !input.manifest_digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(AppError::usage("invalid backfill manifest"));
        }
        let timestamp = now();
        self.connection
            .execute(
                "INSERT OR IGNORE INTO backfills (
                    backfill_id, group_id, node_id, database_instance,
                    baseline_revision, views_fingerprint, manifest_digest,
                    total_chats, state, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'receiving_manifest', ?9, ?9)",
                params![
                    input.backfill_id,
                    input.group_id,
                    input.node_id,
                    input.database_instance,
                    input.baseline_revision,
                    input.views_fingerprint,
                    input.manifest_digest,
                    i64::try_from(input.total_chats)
                        .map_err(|_| AppError::usage("backfill is too large"))?,
                    timestamp
                ],
            )
            .map_err(database)?;
        let backfill = self.backfill(input.backfill_id, input.group_id, input.node_id)?;
        if backfill.database_instance != input.database_instance
            || backfill.baseline_revision != input.baseline_revision
            || backfill.views_fingerprint != input.views_fingerprint
            || backfill.manifest_digest != input.manifest_digest
            || backfill.total_chats != input.total_chats
        {
            return Err(conflict(
                "backfill identity was already used for a different baseline",
            ));
        }
        Ok(backfill)
    }

    pub fn backfill(
        &self,
        backfill_id: &str,
        group_id: &str,
        node_id: &str,
    ) -> Result<Backfill, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT b.database_instance, b.baseline_revision, b.views_fingerprint,
                        b.manifest_digest, b.total_chats, b.state,
                        (SELECT COUNT(*) FROM backfill_members m
                         WHERE m.backfill_id = b.backfill_id AND m.covered = 1)
                 FROM backfills b
                 WHERE b.backfill_id = ?1 AND b.group_id = ?2 AND b.node_id = ?3",
            )
            .map_err(database)?;
        statement
            .query_row(params![backfill_id, group_id, node_id], |row| {
                Ok(Backfill {
                    backfill_id: backfill_id.to_owned(),
                    database_instance: row.get(0)?,
                    baseline_revision: row.get(1)?,
                    views_fingerprint: row.get(2)?,
                    manifest_digest: row.get(3)?,
                    total_chats: row.get::<_, i64>(4)?.try_into().unwrap_or_default(),
                    covered: row.get::<_, i64>(6)?.try_into().unwrap_or_default(),
                    state: row.get(5)?,
                })
            })
            .optional()
            .map_err(database)?
            .ok_or_else(|| conflict("unknown backfill operation"))
    }

    /// Install the verified manifest once its transfer finishes. Repeating the
    /// same manifest is idempotent; different bytes fail closed.
    pub fn store_manifest(
        &self,
        backfill_id: &str,
        group_id: &str,
        node_id: &str,
        members: &[wire::BackfillMember],
    ) -> Result<Backfill, AppError> {
        let backfill = self.backfill(backfill_id, group_id, node_id)?;
        if backfill.total_chats != members.len() as u64 {
            return Err(conflict(
                "backfill manifest member count does not match its declaration",
            ));
        }
        if backfill.state == "receiving_manifest" {
            let transaction = self.connection.unchecked_transaction().map_err(database)?;
            for member in members {
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO backfill_members (
                            backfill_id, chat_id, digest, covered
                         ) VALUES (?1, ?2, ?3, 0)",
                        params![backfill_id, member.chat_id, member.digest],
                    )
                    .map_err(database)?;
            }
            let retained: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM backfill_members WHERE backfill_id = ?1",
                    params![backfill_id],
                    |row| row.get(0),
                )
                .map_err(database)?;
            if retained != i64::try_from(members.len()).unwrap_or(i64::MAX) {
                return Err(conflict(
                    "backfill manifest was already installed with different members",
                ));
            }
            transaction
                .execute(
                    "UPDATE backfills SET state = 'ready', updated_at = ?2
                     WHERE backfill_id = ?1",
                    params![backfill_id, now()],
                )
                .map_err(database)?;
            transaction.commit().map_err(database)?;
        }
        self.backfill(backfill_id, group_id, node_id)
    }

    /// Mark baseline members covered by a committed batch. Members outside the
    /// manifest are live additions and intentionally ignored.
    pub fn cover(
        &self,
        backfill_id: &str,
        group_id: &str,
        node_id: &str,
        members: &[(String, String)],
    ) -> Result<u64, AppError> {
        let backfill = self.backfill(backfill_id, group_id, node_id)?;
        if backfill.state != "ready" && backfill.state != "complete" {
            return Err(conflict("backfill manifest is not ready"));
        }
        let transaction = self.connection.unchecked_transaction().map_err(database)?;
        for (chat_id, digest) in members {
            transaction
                .execute(
                    "UPDATE backfill_members SET covered = 1
                     WHERE backfill_id = ?1 AND chat_id = ?2 AND digest = ?3",
                    params![backfill_id, chat_id, digest],
                )
                .map_err(database)?;
        }
        transaction.commit().map_err(database)?;
        Ok(self.backfill(backfill_id, group_id, node_id)?.covered)
    }

    /// Verify complete manifest coverage. Idempotent after completion.
    pub fn finish_backfill(
        &self,
        backfill_id: &str,
        group_id: &str,
        node_id: &str,
        baseline_revision: i64,
        manifest_digest: &str,
    ) -> Result<Backfill, AppError> {
        let backfill = self.backfill(backfill_id, group_id, node_id)?;
        if backfill.state == "complete" {
            return Ok(backfill);
        }
        if backfill.state != "ready" {
            return Err(conflict("backfill manifest is not complete"));
        }
        if backfill.baseline_revision != baseline_revision
            || backfill.manifest_digest != manifest_digest
        {
            return Err(conflict(
                "backfill finish does not match the installed baseline",
            ));
        }
        if backfill.covered != backfill.total_chats {
            return Err(conflict("backfill baseline coverage is incomplete"));
        }
        self.connection
            .execute(
                "UPDATE backfills SET state = 'complete', updated_at = ?2
                 WHERE backfill_id = ?1",
                params![backfill_id, now()],
            )
            .map_err(database)?;
        self.backfill(backfill_id, group_id, node_id)
    }

    /// Backfill sessions that still accept baseline batches for this node.
    pub fn active_backfills(&self, group_id: &str, node_id: &str) -> Result<Vec<String>, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT backfill_id FROM backfills
                 WHERE group_id = ?1 AND node_id = ?2
                   AND state IN ('ready', 'complete')
                 ORDER BY created_at, backfill_id",
            )
            .map_err(database)?;
        let rows = statement
            .query_map(params![group_id, node_id], |row| row.get::<_, String>(0))
            .map_err(database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database)?;
        Ok(rows)
    }

    /// Record that a newer live publication carries these chats. A
    /// live record at a revision above the frozen baseline satisfies its
    /// baseline member, and a later baseline batch must not overwrite it.
    pub fn record_live(
        &self,
        backfill_id: &str,
        group_id: &str,
        node_id: &str,
        chats: &[String],
        revision: i64,
    ) -> Result<(), AppError> {
        let backfill = self.backfill(backfill_id, group_id, node_id)?;
        if revision <= backfill.baseline_revision {
            return Ok(());
        }
        let transaction = self.connection.unchecked_transaction().map_err(database)?;
        for chat in chats {
            transaction
                .execute(
                    "UPDATE backfill_members
                     SET live_revision = MAX(live_revision, ?2),
                         covered = CASE WHEN ?2 > ?3 THEN 1 ELSE covered END
                     WHERE backfill_id = ?1 AND chat_id = ?4",
                    params![backfill_id, revision, backfill.baseline_revision, chat],
                )
                .map_err(database)?;
        }
        transaction.commit().map_err(database)?;
        Ok(())
    }

    /// Baseline chats already superseded by a live publication at or
    /// above `revision`. Their baseline records must not be applied.
    pub fn superseded(
        &self,
        backfill_id: &str,
        group_id: &str,
        node_id: &str,
        revision: i64,
    ) -> Result<Vec<String>, AppError> {
        self.backfill(backfill_id, group_id, node_id)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT chat_id FROM backfill_members
                 WHERE backfill_id = ?1 AND live_revision > ?2
                 ORDER BY chat_id",
            )
            .map_err(database)?;
        let rows = statement
            .query_map(params![backfill_id, revision], |row| {
                row.get::<_, String>(0)
            })
            .map_err(database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database)?;
        Ok(rows)
    }

    /// Most recently created baseline session for a node.
    pub fn latest_backfill(
        &self,
        group_id: &str,
        node_id: &str,
    ) -> Result<Option<Backfill>, AppError> {
        let id = self
            .connection
            .query_row(
                "SELECT backfill_id FROM backfills
                 WHERE group_id = ?1 AND node_id = ?2
                 ORDER BY created_at DESC, backfill_id DESC LIMIT 1",
                params![group_id, node_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(database)?;
        match id {
            Some(id) => Ok(Some(self.backfill(&id, group_id, node_id)?)),
            None => Ok(None),
        }
    }

    pub fn cancel(&self, operation_id: &str) -> Result<(), AppError> {
        self.set_state(operation_id, State::Cancelled, None, None)?;
        self.remove_pieces(operation_id)
    }

    /// Expire abandoned receiving transfers whose last progress is older than
    /// `cutoff` (milliseconds since the epoch). Completed work is never expired
    /// here: only `receiving` rows release their spool.
    pub fn expire_incomplete(&self, cutoff: i64) -> Result<usize, AppError> {
        // One immediate transaction: an in-flight append on another connection
        // either commits first (and changes `updated_at`) or waits. That keeps a
        // checkpoint acknowledged by `append` from being deleted here.
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(database)?;
        let ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT operation_id FROM operations
                     WHERE state = 'receiving' AND updated_at < ?1",
                )
                .map_err(database)?;
            statement
                .query_map(params![cutoff], |row| row.get::<_, String>(0))
                .map_err(database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(database)?
        };
        let mut expired = Vec::new();
        for id in ids {
            let changed = transaction
                .execute(
                    "UPDATE operations SET state = 'expired', updated_at = ?2
                     WHERE operation_id = ?1 AND state = 'receiving' AND updated_at < ?3",
                    params![id, now(), cutoff],
                )
                .map_err(database)?;
            if changed == 1 {
                expired.push(id);
            }
        }
        for id in &expired {
            transaction
                .execute("DELETE FROM pieces WHERE operation_id = ?1", params![id])
                .map_err(database)?;
        }
        transaction.commit().map_err(database)?;
        // Files go after the durable state change; an interrupted sweep can
        // leave unreferenced piece files, never a checkpoint without bytes.
        for id in &expired {
            match std::fs::remove_dir_all(self.piece_directory(id)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(AppError::io(error)),
            }
        }
        Ok(expired.len())
    }

    /// Forget every transfer, receipt and backfill of one node. A receipt kept
    /// after its history is gone could acknowledge an upload that no longer
    /// exists, so retiring a node removes them with it.
    pub fn retire(&self, group_id: &str, node_id: &str) -> Result<usize, AppError> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )
        .map_err(database)?;
        let ids = {
            let mut statement = transaction
                .prepare("SELECT operation_id FROM operations WHERE group_id = ?1 AND node_id = ?2")
                .map_err(database)?;
            statement
                .query_map(params![group_id, node_id], |row| row.get::<_, String>(0))
                .map_err(database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(database)?
        };
        for id in &ids {
            transaction
                .execute("DELETE FROM pieces WHERE operation_id = ?1", params![id])
                .map_err(database)?;
        }
        transaction
            .execute(
                "DELETE FROM backfill_members WHERE backfill_id IN
                    (SELECT backfill_id FROM backfills WHERE group_id = ?1 AND node_id = ?2)",
                params![group_id, node_id],
            )
            .map_err(database)?;
        for table in ["backfills", "operations"] {
            transaction
                .execute(
                    &format!("DELETE FROM {table} WHERE group_id = ?1 AND node_id = ?2"),
                    params![group_id, node_id],
                )
                .map_err(database)?;
        }
        transaction.commit().map_err(database)?;
        // Files go after the durable change, as when a transfer expires.
        for id in &ids {
            match std::fs::remove_dir_all(self.piece_directory(id)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(AppError::io(error)),
            }
        }
        Ok(ids.len())
    }

    /// Bound retained spool and catalog rows. Published operations keep only
    /// their receipt so a reconnect can still report the outcome.
    pub fn prune(&self, group_id: &str, node_id: &str, keep: usize) -> Result<(), AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT operation_id FROM operations
                 WHERE group_id = ?1 AND node_id = ?2
                   AND state IN ('published', 'cancelled', 'failed', 'expired')
                 ORDER BY updated_at DESC LIMIT -1 OFFSET ?3",
            )
            .map_err(database)?;
        let ids = statement
            .query_map(
                params![group_id, node_id, i64::try_from(keep).unwrap_or(i64::MAX)],
                |row| row.get::<_, String>(0),
            )
            .map_err(database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database)?;
        drop(statement);
        for id in ids {
            self.remove_pieces(&id)?;
            self.connection
                .execute(
                    "DELETE FROM operations WHERE operation_id = ?1",
                    params![id],
                )
                .map_err(database)?;
        }
        Ok(())
    }

    fn pieces(&self, operation_id: &str) -> Result<Vec<wire::Piece>, AppError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT sequence, decoded_offset, decoded_length, digest, compression
                 FROM pieces WHERE operation_id = ?1 ORDER BY sequence",
            )
            .map_err(database)?;
        let rows = statement
            .query_map(params![operation_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i32>(4)?,
                ))
            })
            .map_err(database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database)?;
        drop(statement);
        let directory = self.piece_directory(operation_id);
        rows.into_iter()
            .map(|(sequence, offset, length, digest, compression)| {
                let sequence = u64::try_from(sequence)
                    .map_err(|_| AppError::internal("invalid retained sequence"))?;
                let encoded = std::fs::read(directory.join(format!("{sequence}.zst")))
                    .map_err(|_| AppError::internal("read retained sync piece"))?;
                Ok(wire::Piece {
                    sequence,
                    decoded_offset: u64::try_from(offset)
                        .map_err(|_| AppError::internal("invalid retained offset"))?,
                    decoded_length: u64::try_from(length)
                        .map_err(|_| AppError::internal("invalid retained length"))?,
                    digest,
                    encoded,
                    compression,
                })
            })
            .collect()
    }

    fn piece(&self, operation_id: &str, sequence: u64) -> Result<Option<StoredChunk>, AppError> {
        let row = self
            .connection
            .query_row(
                "SELECT decoded_offset, decoded_length, digest, compression
                 FROM pieces WHERE operation_id = ?1 AND sequence = ?2",
                params![
                    operation_id,
                    i64::try_from(sequence).map_err(|_| AppError::usage("invalid sequence"))?
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i32>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(database)?;
        let Some((offset, length, digest, compression)) = row else {
            return Ok(None);
        };
        Ok(Some(StoredChunk {
            sequence,
            decoded_offset: u64::try_from(offset)
                .map_err(|_| AppError::internal("invalid retained offset"))?,
            decoded_length: u64::try_from(length)
                .map_err(|_| AppError::internal("invalid retained length"))?,
            digest,
            encoded: Vec::new(),
            compression,
        }))
    }

    fn piece_path(&self, operation_id: &str, sequence: u64) -> PathBuf {
        self.piece_directory(operation_id)
            .join(format!("{sequence}.zst"))
    }

    fn piece_directory(&self, operation_id: &str) -> PathBuf {
        self.root
            .join("pieces")
            .join(blake3::hash(operation_id.as_bytes()).to_hex().as_str())
    }

    fn write_piece(&self, operation_id: &str, chunk: &StoredChunk) -> Result<(), AppError> {
        let directory = self.piece_directory(operation_id);
        std::fs::create_dir_all(&directory).map_err(AppError::io)?;
        let path = directory.join(format!("{}.zst", chunk.sequence));
        let existing = std::fs::read(&path).ok();
        if let Some(existing) = existing {
            if existing == chunk.encoded {
                return Ok(());
            }
            return Err(conflict("conflicting retry for a retained sync piece"));
        }
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(AppError::io)?;
        file.write_all(&chunk.encoded).map_err(AppError::io)?;
        file.sync_all().map_err(AppError::io)?;
        drop(file);
        if let Ok(directory) = std::fs::File::open(&directory) {
            let _ = directory.sync_all();
        }
        Ok(())
    }

    fn remove_pieces(&self, operation_id: &str) -> Result<(), AppError> {
        let directory = self.piece_directory(operation_id);
        match std::fs::remove_dir_all(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(AppError::io(error)),
        }
        self.connection
            .execute(
                "DELETE FROM pieces WHERE operation_id = ?1",
                params![operation_id],
            )
            .map_err(database)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_catalog_rejects_other_schemas_without_modification() {
        for version in [0, SCHEMA_VERSION + 1] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("catalog.sqlite3");
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES('retained');",
                )
                .unwrap();
            connection
                .pragma_update(None, "user_version", version)
                .unwrap();
            drop(connection);
            let before = std::fs::read(&path).unwrap();
            let error = Catalog::open(root.path())
                .err()
                .expect("foreign schema rejected");
            assert!(
                error
                    .error
                    .message
                    .contains("unsupported sync catalog schema")
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
    }

    fn manifest(bytes: u64, pieces: &[(u64, u64, &str)]) -> String {
        let entries = pieces
            .iter()
            .map(|(sequence, offset, digest)| wire::ManifestPiece {
                sequence: *sequence,
                decoded_offset: *offset,
                decoded_length: 2,
                digest,
            })
            .collect::<Vec<_>>();
        wire::manifest_digest(bytes, 2, &entries)
    }

    fn begin(catalog: &Catalog, id: &str, manifest: &str, bytes: u64) -> Operation {
        match catalog
            .begin(&Begin {
                operation_id: id,
                group_id: "group",
                node_id: "node",
                kind: "publication",
                manifest_digest: manifest,
                decoded_bytes: bytes,
                record_count: 2,
                start_json: "{\"type\":\"start\"}",
            })
            .unwrap()
        {
            BeginOutcome::New(operation) | BeginOutcome::Existing(operation) => operation,
        }
    }

    fn chunk(sequence: u64, offset: u64, digest: &str, encoded: &[u8]) -> StoredChunk {
        StoredChunk {
            sequence,
            decoded_offset: offset,
            decoded_length: 2,
            digest: digest.into(),
            encoded: encoded.to_vec(),
            compression: 1,
        }
    }

    fn backfill(catalog: &Catalog, id: &str, manifest: &str, total: u64) -> Backfill {
        catalog
            .begin_backfill(&BackfillBegin {
                backfill_id: id,
                group_id: "group",
                node_id: "node",
                database_instance: "instance",
                baseline_revision: 7,
                views_fingerprint: "generation",
                manifest_digest: manifest,
                total_chats: total,
            })
            .unwrap();
        catalog.backfill(id, "group", "node").unwrap()
    }

    #[test]
    fn retiring_a_node_forgets_its_transfers_and_backfills_but_no_other_nodes() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let digest = "a".repeat(64);
        begin(&catalog, "mine", &manifest(2, &[(0, 0, &digest)]), 2);
        backfill(&catalog, "backfill", &digest, 2);
        let other = Begin {
            operation_id: "theirs",
            group_id: "group",
            node_id: "other",
            kind: "publication",
            manifest_digest: &manifest(2, &[(0, 0, &digest)]),
            decoded_bytes: 2,
            record_count: 2,
            start_json: "{\"type\":\"start\"}",
        };
        catalog.begin(&other).unwrap();
        // A spooled piece directory, as an interrupted upload leaves behind.
        let spool = catalog.piece_directory("mine");
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::write(spool.join("0.zst"), b"piece").unwrap();

        assert_eq!(catalog.retire("group", "node").unwrap(), 1);
        let count = |table: &str, node: &str| -> i64 {
            catalog
                .connection
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE node_id = ?1"),
                    params![node],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(count("operations", "node"), 0);
        assert_eq!(count("backfills", "node"), 0);
        assert_eq!(count("operations", "other"), 1);
        assert!(!spool.exists());
        // Nothing left to retire is not an error.
        assert_eq!(catalog.retire("group", "node").unwrap(), 0);
    }

    #[test]
    fn backfill_requires_the_declared_members_and_complete_coverage() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let manifest = "a".repeat(64);
        let opened = backfill(&catalog, "backfill", &manifest, 2);
        assert_eq!(opened.state, "receiving_manifest");
        assert_eq!(opened.covered, 0);

        // A manifest with the wrong member count cannot be installed.
        assert!(
            catalog
                .store_manifest(
                    "backfill",
                    "group",
                    "node",
                    &[wire::BackfillMember {
                        chat_id: "a".into(),
                        digest: "b".repeat(64),
                    }],
                )
                .is_err()
        );

        let members = [
            wire::BackfillMember {
                chat_id: "a".into(),
                digest: "b".repeat(64),
            },
            wire::BackfillMember {
                chat_id: "b".into(),
                digest: "c".repeat(64),
            },
        ];
        let ready = catalog
            .store_manifest("backfill", "group", "node", &members)
            .unwrap();
        assert_eq!(ready.state, "ready");
        // Repeating the same manifest is idempotent.
        assert_eq!(
            catalog
                .store_manifest("backfill", "group", "node", &members)
                .unwrap(),
            ready
        );
        // Finish before coverage fails closed.
        assert!(
            catalog
                .finish_backfill("backfill", "group", "node", 7, &manifest)
                .is_err()
        );

        // A digest that does not match the frozen member is not evidence.
        let stale = catalog
            .cover("backfill", "group", "node", &[("a".into(), "d".repeat(64))])
            .unwrap();
        assert_eq!(stale, 0);

        let covered = catalog
            .cover(
                "backfill",
                "group",
                "node",
                &[
                    ("a".into(), "b".repeat(64)),
                    ("extra".into(), "e".repeat(64)),
                ],
            )
            .unwrap();
        assert_eq!(covered, 1, "unknown live members are not baseline evidence");
        assert!(
            catalog
                .finish_backfill("backfill", "group", "node", 7, &manifest)
                .is_err(),
            "one uncovered member still blocks completion"
        );

        catalog
            .cover("backfill", "group", "node", &[("b".into(), "c".repeat(64))])
            .unwrap();
        let complete = catalog
            .finish_backfill("backfill", "group", "node", 7, &manifest)
            .unwrap();
        assert_eq!(complete.state, "complete");
        assert_eq!(complete.covered, 2);
    }

    #[test]
    fn live_publications_supersede_baseline_members() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let manifest = "a".repeat(64);
        catalog
            .begin_backfill(&BackfillBegin {
                backfill_id: "backfill",
                group_id: "group",
                node_id: "node",
                database_instance: "instance",
                baseline_revision: 5,
                views_fingerprint: "generation",
                manifest_digest: &manifest,
                total_chats: 2,
            })
            .unwrap();
        let members = [
            wire::BackfillMember {
                chat_id: "a".into(),
                digest: "b".repeat(64),
            },
            wire::BackfillMember {
                chat_id: "b".into(),
                digest: "c".repeat(64),
            },
        ];
        catalog
            .store_manifest("backfill", "group", "node", &members)
            .unwrap();

        // A live record above the frozen baseline covers its baseline member.
        catalog
            .record_live("backfill", "group", "node", &["a".into()], 6)
            .unwrap();
        let backfill = catalog.backfill("backfill", "group", "node").unwrap();
        assert_eq!(backfill.covered, 1);
        assert_eq!(
            catalog.superseded("backfill", "group", "node", 5).unwrap(),
            vec!["a".to_owned()]
        );
        // A batch at the live revision already contains the newer state.
        assert_eq!(
            catalog.superseded("backfill", "group", "node", 6).unwrap(),
            Vec::<String>::new()
        );
        // A live record at or below the baseline proves nothing.
        catalog
            .record_live("backfill", "group", "node", &["b".into()], 5)
            .unwrap();
        assert_eq!(
            catalog.superseded("backfill", "group", "node", 5).unwrap(),
            vec!["a".to_owned()]
        );
        assert_eq!(
            catalog
                .backfill("backfill", "group", "node")
                .unwrap()
                .covered,
            1
        );
    }

    #[test]
    fn backfill_identity_is_immutable_across_sessions() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let manifest = "a".repeat(64);
        backfill(&catalog, "backfill", &manifest, 1);
        // Same identity with a different baseline revision conflicts.
        assert!(
            catalog
                .begin_backfill(&BackfillBegin {
                    backfill_id: "backfill",
                    group_id: "group",
                    node_id: "node",
                    database_instance: "instance",
                    baseline_revision: 8,
                    views_fingerprint: "generation",
                    manifest_digest: &manifest,
                    total_chats: 1,
                })
                .is_err()
        );
        // Another node cannot read this session.
        assert!(catalog.backfill("backfill", "group", "other").is_err());
    }

    /// Model-based property test: random operation sequences must preserve the
    /// catalog invariants the TLA+ models state. Independent expected state.
    #[allow(
        clippy::too_many_lines,
        reason = "one model-based sequence applies and checks every catalog transition"
    )]
    #[test]
    fn random_operation_sequences_preserve_catalog_invariants() {
        use proptest::prelude::*;

        #[derive(Clone, Copy, Debug)]
        enum Op {
            Begin,
            AppendNext,
            DuplicateLast,
            WrongDigest,
            Gap,
            Finish,
            Expire,
            Reopen,
            Cancel,
        }

        fn op_strategy() -> impl Strategy<Value = Op> {
            prop::sample::select(vec![
                Op::Begin,
                Op::AppendNext,
                Op::AppendNext,
                Op::DuplicateLast,
                Op::WrongDigest,
                Op::Gap,
                Op::Finish,
                Op::Expire,
                Op::Reopen,
                Op::Cancel,
            ])
        }

        proptest!(|(ops in prop::collection::vec(op_strategy(), 1..40))| {
            let directory = tempfile::tempdir().unwrap();
            let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
            let mut catalog = Catalog::open(directory.path()).unwrap();
            let mut begun = false;
            let mut next = 0u64;
            for op in ops {
                match op {
                    Op::Begin if !begun => {
                        begin(&catalog, "op", &digest, 4);
                        begun = true;
                    }
                    Op::Begin => {
                        // Re-begin of the same bytes is idempotent.
                        begin(&catalog, "op", &digest, 4);
                    }
                    Op::AppendNext if begun && next < 2 && receiving(&catalog) => {
                        let seq = next;
                        let offset = seq * 2;
                        let digest = if seq == 0 { "a" } else { "b" };
                        let bytes: &[u8] = if seq == 0 { b"first" } else { b"second" };
                        catalog
                            .append("op", "group", "node", &chunk(seq, offset, digest, bytes))
                            .unwrap();
                        next += 1;
                    }
                    Op::DuplicateLast if begun && next > 0 && receiving(&catalog) => {
                        let seq = next - 1;
                        let digest = if seq == 0 { "a" } else { "b" };
                        let bytes: &[u8] = if seq == 0 { b"first" } else { b"second" };
                        catalog
                            .append("op", "group", "node", &chunk(seq, seq * 2, digest, bytes))
                            .unwrap();
                    }
                    // A retry of a retained sequence with different bytes.
                    Op::WrongDigest if begun && next > 0 && receiving(&catalog) => {
                        let seq = next - 1;
                        let rejected = catalog.append(
                            "op",
                            "group",
                            "node",
                            &chunk(seq, seq * 2, "wrong", b"xx"),
                        );
                        assert!(rejected.is_err(), "changed retry bytes must fail closed");
                    }
                    // Skipping the next expected sequence.
                    Op::Gap if begun && next == 0 && receiving(&catalog) => {
                        let rejected =
                            catalog.append("op", "group", "node", &chunk(1, 2, "b", b"second"));
                        assert!(rejected.is_err(), "gaps must fail closed");
                    }
                    Op::Finish if begun && next == 2 && receiving(&catalog) => {
                        catalog.finish("op", "group", "node", &digest).unwrap();
                    }
                    Op::Expire => {
                        catalog.expire_incomplete(now() + 1).unwrap();
                    }
                    Op::Reopen => {
                        catalog = Catalog::open(directory.path()).unwrap();
                    }
                    Op::Cancel if begun => {
                        catalog.cancel("op").unwrap();
                    }
                    _ => {}
                }

                if !begun {
                    continue;
                }
                // Invariants: checkpoint equals retained pieces, and terminal
                // states never keep a spool.
                let operation = catalog.select("op", "group", "node").unwrap();
                assert_eq!(
                    operation.next_sequence, next,
                    "checkpoint must equal the retained pieces"
                );
                assert_eq!(operation.decoded_offset, next * 2);
                let spool = directory
                    .path()
                    .join("pieces")
                    .join(blake3::hash(b"op").to_hex().as_str());
                if matches!(operation.state, State::Expired | State::Cancelled) {
                    assert!(!spool.exists(), "terminal states must release their spool");
                    assert!(
                        !catalog
                            .recoverable()
                            .unwrap()
                            .iter()
                            .any(|row| row.operation_id == "op")
                    );
                }
                if operation.state == State::Published {
                    assert!(operation.revision.is_some());
                }
            }
        });
    }

    fn receiving(catalog: &Catalog) -> bool {
        catalog
            .select("op", "group", "node")
            .is_ok_and(|operation| operation.state == State::Receiving)
    }

    #[test]
    fn spool_quota_rejects_a_piece_and_keeps_the_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
        // Two pieces of two encoded bytes fit in three bytes only once.
        let catalog = Catalog::open_with_limit(directory.path(), 3).unwrap();
        begin(&catalog, "op", &digest, 4);
        catalog
            .append("op", "group", "node", &chunk(0, 0, "a", b"xx"))
            .unwrap();
        let rejected = catalog.append("op", "group", "node", &chunk(1, 2, "b", b"yy"));
        assert!(
            rejected.is_err(),
            "the spool ceiling rejects the second piece"
        );
        assert_eq!(
            catalog.select("op", "group", "node").unwrap().next_sequence,
            1
        );
        let count: i64 = catalog
            .connection
            .query_row(
                "SELECT COUNT(*) FROM pieces WHERE operation_id = 'op' AND sequence = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert!(
            !directory
                .path()
                .join("pieces")
                .join(blake3::hash(b"op").to_hex().as_str())
                .join("1.zst")
                .exists(),
            "a rejected piece must not leave a file behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_spool_reports_an_io_failure_without_advancing() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
        let catalog = Catalog::open(directory.path()).unwrap();
        begin(&catalog, "op", &digest, 4);
        let spool = directory
            .path()
            .join("pieces")
            .join(blake3::hash(b"op").to_hex().as_str());
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o500)).unwrap();
        let rejected = catalog.append("op", "group", "node", &chunk(0, 0, "a", b"xx"));
        std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(rejected.is_err(), "an unwritable spool fails closed");
        assert_eq!(
            catalog.select("op", "group", "node").unwrap().next_sequence,
            0
        );
    }
    #[test]
    fn an_expired_transfer_rejects_further_pieces_from_another_connection() {
        let directory = tempfile::tempdir().unwrap();
        let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
        let writer = Catalog::open(directory.path()).unwrap();
        begin(&writer, "op", &digest, 4);
        writer
            .append("op", "group", "node", &chunk(0, 0, "a", b"first"))
            .unwrap();

        // A second connection, like another /v1/sync session, expires the row.
        let sweeper = Catalog::open(directory.path()).unwrap();
        assert_eq!(sweeper.expire_incomplete(now() + 1).unwrap(), 1);
        assert_eq!(
            writer.select("op", "group", "node").unwrap().state,
            State::Expired
        );

        // The writer's next piece must fail closed instead of acknowledging
        // bytes that the sweep already deleted.
        let rejected = writer.append("op", "group", "node", &chunk(1, 2, "b", b"second"));
        assert!(rejected.is_err(), "expired transfers cannot accept pieces");
        assert_eq!(
            writer.select("op", "group", "node").unwrap().decoded_offset,
            2
        );

        // No piece row or file survives for the rejected sequence.
        let count: i64 = writer
            .connection
            .query_row(
                "SELECT COUNT(*) FROM pieces WHERE operation_id = 'op' AND sequence = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert!(
            !directory
                .path()
                .join("pieces")
                .join(blake3::hash(b"op").to_hex().as_str())
                .join("1.zst")
                .exists()
        );
    }

    #[test]
    fn abandoned_receiving_transfers_expire_and_release_their_spool() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
        begin(&catalog, "abandoned", &digest, 4);
        catalog
            .append("abandoned", "group", "node", &chunk(0, 0, "a", b"first"))
            .unwrap();
        // A completed receipt is not an abandoned receiving transfer.
        begin(&catalog, "queued", &digest, 4);
        for (sequence, offset, digest) in [(0u64, 0u64, "a"), (1, 2, "b")] {
            catalog
                .append(
                    "queued",
                    "group",
                    "node",
                    &chunk(sequence, offset, digest, b"xx"),
                )
                .unwrap();
        }
        catalog.finish("queued", "group", "node", &digest).unwrap();

        // A cutoff before any progress expires nothing.
        assert_eq!(catalog.expire_incomplete(0).unwrap(), 0);
        assert_eq!(
            catalog.select("abandoned", "group", "node").unwrap().state,
            State::Receiving
        );

        // A cutoff after the last update expires only the receiving row.
        assert_eq!(catalog.expire_incomplete(now() + 1).unwrap(), 1);
        assert_eq!(
            catalog.select("abandoned", "group", "node").unwrap().state,
            State::Expired
        );
        assert_eq!(
            catalog.select("queued", "group", "node").unwrap().state,
            State::QueuedForApply
        );
        // The spool directory is gone, so a reconnect cannot resume it.
        assert!(
            !directory
                .path()
                .join("pieces")
                .join(blake3::hash(b"abandoned").to_hex().as_str())
                .exists()
        );
        assert!(
            !catalog
                .recoverable()
                .unwrap()
                .iter()
                .any(|row| row.operation_id == "abandoned")
        );
    }

    #[test]
    fn restart_preserves_checkpoint_and_exact_retained_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
        begin(&catalog, "op", &digest, 4);
        let operation = catalog
            .append("op", "group", "node", &chunk(0, 0, "a", b"first"))
            .unwrap();
        assert_eq!(operation.next_sequence, 1);
        assert_eq!(operation.decoded_offset, 2);
        drop(catalog);

        let reopened = Catalog::open(directory.path()).unwrap();
        let operation = reopened.select("op", "group", "node").unwrap();
        assert_eq!(operation.next_sequence, 1);
        assert_eq!(operation.decoded_offset, 2);
        // Replaying the retained piece with the same digest is idempotent.
        let operation = reopened
            .append("op", "group", "node", &chunk(0, 0, "a", b"first"))
            .unwrap();
        assert_eq!(operation.next_sequence, 1);
        // The next piece continues from the persisted offset.
        let operation = reopened
            .append("op", "group", "node", &chunk(1, 2, "b", b"second"))
            .unwrap();
        assert_eq!(operation.next_sequence, 2);
        assert_eq!(operation.decoded_offset, 4);
        assert!(reopened.finish("op", "group", "node", &digest).is_ok());
    }

    #[test]
    fn conflicting_retries_gaps_and_foreign_owners_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let digest = manifest(6, &[(0, 0, "a"), (1, 2, "b"), (2, 4, "c")]);
        begin(&catalog, "op", &digest, 6);
        catalog
            .append("op", "group", "node", &chunk(0, 0, "a", b"first"))
            .unwrap();
        assert!(
            catalog
                .append("op", "group", "node", &chunk(0, 0, "b", b"other"))
                .is_err()
        );
        assert!(
            catalog
                .append("op", "group", "node", &chunk(2, 4, "c", b"third"))
                .is_err()
        );
        assert!(
            catalog
                .append("op", "group", "other", &chunk(1, 2, "c", b"third"))
                .is_err()
        );
        assert!(
            catalog
                .begin(&Begin {
                    operation_id: "op",
                    group_id: "group",
                    node_id: "node",
                    kind: "publication",
                    manifest_digest: "changed",
                    decoded_bytes: 6,
                    record_count: 2,
                    start_json: "{}",
                })
                .is_err()
        );
    }

    #[test]
    fn incomplete_or_tampered_receipts_never_queue_for_apply() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
        begin(&catalog, "op", &digest, 4);
        catalog
            .append("op", "group", "node", &chunk(0, 0, "a", b"first"))
            .unwrap();
        assert!(catalog.finish("op", "group", "node", &digest).is_err());
        catalog
            .append("op", "group", "node", &chunk(1, 2, "b", b"second"))
            .unwrap();
        assert!(catalog.finish("op", "group", "node", "other").is_err());
        let (operation, pieces) = catalog.finish("op", "group", "node", &digest).unwrap();
        assert_eq!(operation.state, State::QueuedForApply);
        assert_eq!(pieces.len(), 2);
    }

    #[test]
    fn published_receipts_survive_restart_without_retaining_piece_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path()).unwrap();
        let digest = manifest(4, &[(0, 0, "a"), (1, 2, "b")]);
        begin(&catalog, "op", &digest, 4);
        catalog
            .append("op", "group", "node", &chunk(0, 0, "a", b"first"))
            .unwrap();
        catalog
            .append("op", "group", "node", &chunk(1, 2, "b", b"second"))
            .unwrap();
        catalog.finish("op", "group", "node", &digest).unwrap();
        catalog.published("op", 9, "receipt").unwrap();
        drop(catalog);
        let reopened = Catalog::open(directory.path()).unwrap();
        let operation = reopened.select("op", "group", "node").unwrap();
        assert_eq!(operation.state, State::Published);
        assert_eq!(operation.revision, Some(9));
        assert_eq!(operation.receipt_digest.as_deref(), Some("receipt"));
        assert!(
            reopened
                .append("op", "group", "node", &chunk(2, 4, "c", b"third"))
                .is_err()
        );
        assert!(reopened.finish("op", "group", "node", &digest).is_err());
    }
}
