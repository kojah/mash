"""Read-only Apple Notes over a copied NoteStore.sqlite.

A note is a Document (authorless, standalone text). Apple stores the body in
`ZICNOTEDATA.ZDATA` as gzip of a protobuf whose text sits at
document(2).note(3).note_text(2); the walk below follows that path and returns
empty text rather than guessing when a blob does not match. Callers copy the
database (plus -wal/-shm) before opening it; nothing here writes.

The path was checked against a real macOS 26 NoteStore (805 notes); the tests
use synthetic blobs of that shape, since real ones hold private notes.
"""

import gzip
import sqlite3
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path

from mash_sdk import Object, Role

APPLE_EPOCH = datetime(2001, 1, 1, tzinfo=UTC)
KIND = "apple-notes:note"


@dataclass(frozen=True)
class Settings:
    database: Path
    account: str

    @property
    def source_id(self) -> str:
        return f"apple-notes:{self.account}"


def _varint(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    shift = 0
    while True:
        if offset >= len(data):
            raise ValueError("truncated varint")
        byte = data[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return value, offset
        shift += 7
        if shift > 63:
            raise ValueError("varint too long")


def field(data: bytes, number: int) -> bytes | None:
    """The first length-delimited payload of `number`, or None."""
    offset = 0
    while offset < len(data):
        key, offset = _varint(data, offset)
        tag, wire = key >> 3, key & 7
        if wire == 2:
            length, offset = _varint(data, offset)
            value = data[offset : offset + length]
            offset += length
            if tag == number:
                return value
        elif wire == 0:
            _, offset = _varint(data, offset)
        elif wire == 1:
            offset += 8
        elif wire == 5:
            offset += 4
        else:
            raise ValueError(f"unsupported wire type {wire}")
    return None


def note_text(blob: bytes) -> str:
    """Decode document(2) -> note(3) -> note_text(2)."""
    try:
        raw = gzip.decompress(blob)
    except OSError:
        return ""
    document = field(raw, 2)
    if document is None:
        return ""
    note = field(document, 3)
    if note is None:
        return ""
    text = field(note, 2)
    return text.decode("utf-8", "replace") if text else ""


def _moment(value: float | None) -> int | None:
    if value is None:
        return None
    return int((APPLE_EPOCH + timedelta(seconds=value)).timestamp() * 1000)


def _newest(columns: set[str], table: str, *names: str) -> str:
    """The first of `names` this database has, as SQL; NULL if none."""
    present = [f"{table}.{name}" for name in names if name in columns]
    return (
        f"COALESCE({', '.join(present)})"
        if len(present) > 1
        else (present or ["NULL"])[0]
    )


def notes(settings: Settings) -> list[Object]:
    connection = sqlite3.connect(f"file:{settings.database}?mode=ro", uri=True)
    connection.row_factory = sqlite3.Row
    try:
        # macOS renames these columns between releases; use the newest present.
        columns = {
            row[1]
            for row in connection.execute("PRAGMA table_info(ZICCLOUDSYNCINGOBJECT)")
        }
        created = _newest(
            columns, "n", "ZCREATIONDATE3", "ZCREATIONDATE1", "ZCREATIONDATE"
        )
        modified = _newest(columns, "n", "ZMODIFICATIONDATE1", "ZMODIFICATIONDATE")
        folder = _newest(columns, "f", "ZTITLE2", "ZTITLE1")
        rows = connection.execute(
            "SELECT n.Z_PK, n.ZIDENTIFIER, n.ZTITLE1,"
            f" {created} AS ZCREATIONDATE, {modified} AS ZMODIFICATIONDATE,"
            f" {folder} AS FOLDER, d.ZDATA FROM ZICCLOUDSYNCINGOBJECT n"
            " LEFT JOIN ZICNOTEDATA d ON d.ZNOTE = n.Z_PK"
            " LEFT JOIN ZICCLOUDSYNCINGOBJECT f ON f.Z_PK = n.ZFOLDER"
            " WHERE d.Z_PK IS NOT NULL"
        ).fetchall()
    finally:
        connection.close()
    found = []
    for row in rows:
        identifier = row["ZIDENTIFIER"] or str(row["Z_PK"])
        found.append(
            Object(
                role=Role.document,
                kind=KIND,
                id=identifier,
                title=row["ZTITLE1"] or None,
                content=note_text(row["ZDATA"]) if row["ZDATA"] is not None else "",
                created_at=_moment(row["ZCREATIONDATE"]),
                updated_at=_moment(row["ZMODIFICATIONDATE"]),
                metadata={"folder": row["FOLDER"]},
            )
        )
    return found
