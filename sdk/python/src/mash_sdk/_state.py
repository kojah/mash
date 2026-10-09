"""A durable client-side pending batch per source/destination, using SQLite.

An HTTP timeout leaves the exact bytes and batch ID pending. Only a matching
silver receipt advances the checkpoint. Server-side once-only application still
requires transactional receipt storage at the ingestion boundary.
"""

import os
import sqlite3
from dataclasses import dataclass
from importlib.resources import files
from pathlib import Path
from types import TracebackType
from typing import Self
from uuid import uuid4

import orjson
from pydantic import BaseModel, ConfigDict, JsonValue, TypeAdapter, ValidationError

from ._generated.batch import Batch
from ._generated.receipt import Receipt
from ._receipts import decode_receipt
from .changes import Changes
from .codec import Contract, encode_batch, finite

APPLICATION_ID = 0x4D53444B
STATE_VERSION = 1


class StateError(RuntimeError):
    """An invalid state transition; never contains canonical record text."""


class PendingExists(StateError):
    """Deliver the pending batch before preparing another batch for this scope."""


class ScopeRow(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    revision: str
    cursor: bytes | None
    last_receipt: bytes | None


class PendingRow(BaseModel):
    model_config = ConfigDict(strict=True, extra="forbid")
    batch_id: str
    expected_revision: str
    payload: bytes
    next_cursor: bytes | None


@dataclass(frozen=True)
class PendingBatch:
    source_id: str
    destination_id: str
    batch_id: str
    expected_revision: int
    payload: bytes


@dataclass(frozen=True)
class Checkpoint:
    revision: int
    cursor: JsonValue
    receipt: Receipt | None


def revision_value(value: str) -> int:
    if not value.isascii() or not value.isdecimal() or len(value) > 20:
        raise StateError("stored revision is invalid")
    result = int(value)
    if result >= 1 << 64:
        raise StateError("stored revision is invalid")
    return result


class StateStore:
    """One private SDK state file can serve several destinations and sources.

    Callers own this synchronous resource. Async runners should put its brief
    disk operations on their dedicated blocking worker rather than the event loop.
    The source cursor is opaque JSON, not a credential or a database connection.
    """

    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        try:
            descriptor = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError:
            pass
        else:
            os.close(descriptor)
        self.connection = sqlite3.connect(path, timeout=10, isolation_level=None)
        self.connection.row_factory = sqlite3.Row
        try:
            self._initialize()
            self.contract = Contract.bundled()
        except BaseException:
            # Initialization owns the connection even if it fails or is cancelled.
            self.connection.close()
            raise

    def _initialize(self) -> None:
        self.connection.execute("BEGIN IMMEDIATE")
        try:
            application_id = self.connection.execute(
                "PRAGMA application_id"
            ).fetchone()[0]
            version = self.connection.execute("PRAGMA user_version").fetchone()[0]
            tables = self.connection.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
            if application_id == 0 and version == 0 and not tables:
                schema = files("mash_sdk").joinpath("state.sql").read_text()
                for statement in schema.split(";"):
                    if statement.strip():
                        self.connection.execute(statement)
                self.connection.execute(f"PRAGMA application_id = {APPLICATION_ID}")
                self.connection.execute(f"PRAGMA user_version = {STATE_VERSION}")
            elif application_id != APPLICATION_ID or version != STATE_VERSION:
                raise StateError("state database is not a supported SDK state file")
            self.connection.execute("COMMIT")
        except BaseException:
            if self.connection.in_transaction:
                self.connection.execute("ROLLBACK")
            raise
        self.connection.execute("PRAGMA foreign_keys = ON")
        self.connection.execute("PRAGMA journal_mode = WAL")
        self.connection.execute("PRAGMA synchronous = FULL")

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.connection.close()

    def _scope(self, destination_id: str, source_id: str) -> ScopeRow | None:
        row = self.connection.execute(
            "SELECT revision,cursor,last_receipt FROM scopes "
            "WHERE destination_id=? AND source_id=?",
            (destination_id, source_id),
        ).fetchone()
        if row is None:
            return None
        try:
            return ScopeRow.model_validate(dict(row))
        except ValidationError:
            raise StateError("stored checkpoint has invalid fields") from None

    def pending(self, *, destination_id: str, source_id: str) -> PendingBatch | None:
        row = self.connection.execute(
            "SELECT batch_id,expected_revision,payload,next_cursor FROM pending "
            "WHERE destination_id=? AND source_id=?",
            (destination_id, source_id),
        ).fetchone()
        if row is None:
            return None
        try:
            value = PendingRow.model_validate(dict(row))
            batch = Batch.model_validate_json(value.payload)
        except ValidationError:
            raise StateError("stored pending batch has invalid fields") from None
        expected = revision_value(value.expected_revision)
        if (
            batch.source_id != source_id
            or batch.batch_id != value.batch_id
            or batch.expected_revision != expected
        ):
            raise StateError("stored payload does not match its delivery identity")
        return PendingBatch(
            source_id,
            destination_id,
            value.batch_id,
            expected,
            value.payload,
        )

    def checkpoint(self, *, destination_id: str, source_id: str) -> Checkpoint:
        row = self._scope(destination_id, source_id)
        if row is None:
            return Checkpoint(0, None, None)
        try:
            cursor = (
                None
                if row.cursor is None
                else TypeAdapter(JsonValue).validate_python(orjson.loads(row.cursor))
            )
        except (orjson.JSONDecodeError, ValidationError):
            raise StateError("stored cursor is invalid") from None
        receipt = None if row.last_receipt is None else decode_receipt(row.last_receipt)
        revision = revision_value(row.revision)
        if receipt is not None and (
            receipt.source_id != source_id or receipt.source_revision != revision
        ):
            raise StateError("stored receipt does not match its checkpoint")
        return Checkpoint(revision, cursor, receipt)

    def prepare(
        self,
        changes: Changes,
        *,
        destination_id: str,
        source_id: str,
        next_cursor: JsonValue = None,
    ) -> PendingBatch:
        """Commit a batch before attempting network delivery; serialize writers."""
        self.connection.execute("BEGIN IMMEDIATE")
        try:
            if (
                self.pending(destination_id=destination_id, source_id=source_id)
                is not None
            ):
                raise PendingExists(
                    "a batch is already pending for this source and destination"
                )
            checkpoint = self.checkpoint(
                destination_id=destination_id, source_id=source_id
            )
            batch = changes.to_batch(
                source_id=source_id,
                batch_id=str(uuid4()),
                expected_revision=checkpoint.revision,
            )
            payload = encode_batch(batch, contract=self.contract)
            cursor_value = TypeAdapter(JsonValue).validate_python(
                checkpoint.cursor if next_cursor is None else next_cursor
            )
            if not finite(cursor_value):
                raise StateError("source cursor contains a non-finite number")
            cursor = orjson.dumps(cursor_value)
            if len(cursor) > self.contract.limits.metadata_bytes:
                raise StateError("source cursor exceeds its size limit")
            self.connection.execute(
                "INSERT OR IGNORE INTO scopes(destination_id,source_id,revision) "
                "VALUES(?,?,?)",
                (destination_id, source_id, str(checkpoint.revision)),
            )
            self.connection.execute(
                "INSERT INTO pending(destination_id,source_id,batch_id,"
                "expected_revision,payload,next_cursor) VALUES(?,?,?,?,?,?)",
                (
                    destination_id,
                    source_id,
                    batch.batch_id,
                    str(checkpoint.revision),
                    payload,
                    cursor,
                ),
            )
            self.connection.execute("COMMIT")
            return PendingBatch(
                source_id, destination_id, batch.batch_id, checkpoint.revision, payload
            )
        except BaseException:
            if self.connection.in_transaction:
                self.connection.execute("ROLLBACK")
            raise

    def acknowledge(
        self, payload: bytes, *, destination_id: str, source_id: str
    ) -> Receipt:
        """Advance on a matching silver receipt, even if gold is pending."""
        receipt = decode_receipt(payload)
        if receipt.source_id != source_id:
            raise StateError("receipt belongs to another source")
        self.connection.execute("BEGIN IMMEDIATE")
        try:
            pending = self.pending(destination_id=destination_id, source_id=source_id)
            checkpoint = self.checkpoint(
                destination_id=destination_id, source_id=source_id
            )
            if pending is None:
                if checkpoint.receipt == receipt:
                    self.connection.execute("COMMIT")
                    return receipt
                raise StateError("receipt does not match a pending batch")
            if (
                receipt.batch_id != pending.batch_id
                or receipt.source_revision != pending.expected_revision + 1
            ):
                raise StateError(
                    "receipt identity or revision does not match the pending batch"
                )
            if (
                checkpoint.receipt is not None
                and receipt.silver_revision < checkpoint.receipt.silver_revision
            ):
                raise StateError("receipt silver revision moved backwards")
            self.connection.execute(
                "UPDATE scopes SET revision=?,cursor=(SELECT next_cursor FROM "
                "pending WHERE destination_id=? AND source_id=?),last_receipt=? "
                "WHERE destination_id=? AND source_id=?",
                (
                    str(receipt.source_revision),
                    destination_id,
                    source_id,
                    payload,
                    destination_id,
                    source_id,
                ),
            )
            self.connection.execute(
                "DELETE FROM pending WHERE destination_id=? AND source_id=?",
                (destination_id, source_id),
            )
            self.connection.execute("COMMIT")
            return receipt
        except BaseException:
            if self.connection.in_transaction:
                self.connection.execute("ROLLBACK")
            raise
