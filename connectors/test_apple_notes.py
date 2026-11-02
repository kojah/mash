"""Apple Notes reader over a synthetic NoteStore copy."""

import gzip
import sqlite3
import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path

from connectors.apple_notes import Settings, note_text, notes

APPLE = datetime(2001, 1, 1, tzinfo=UTC)


def varint(value: int) -> bytes:
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def string_field(number: int, text: str | bytes) -> bytes:
    payload = text.encode() if isinstance(text, str) else text
    return varint(number << 3 | 2) + varint(len(payload)) + payload


def note_blob(text: str) -> bytes:
    note = string_field(2, text)
    document = string_field(2, note)
    return gzip.compress(string_field(2, document))


SCHEMA = """
CREATE TABLE ZICCLOUDSYNCINGOBJECT (
  Z_PK INTEGER PRIMARY KEY, ZIDENTIFIER TEXT, ZTITLE1 TEXT,
  ZCREATIONDATE REAL, ZMODIFICATIONDATE REAL, ZFOLDER INTEGER);
CREATE TABLE ZICNOTEDATA (Z_PK INTEGER PRIMARY KEY, ZNOTE INTEGER, ZDATA BLOB);
"""


class AppleNotesTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        path = Path(temporary.name) / "NoteStore.sqlite"
        con = sqlite3.connect(path)
        con.executescript(SCHEMA)
        con.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT VALUES (1,'folder','Notes',0,0,NULL)"
        )
        con.execute(
            "INSERT INTO ZICCLOUDSYNCINGOBJECT VALUES (2,'note-a','Shopping',10,20,1)"
        )
        con.execute("INSERT INTO ZICNOTEDATA VALUES (1,2,?)", (note_blob("milk and eggs"),))
        con.commit()
        con.close()
        self.settings = Settings(database=path, account="james")

    def test_the_body_is_decoded_from_the_protobuf(self) -> None:
        (note,) = notes(self.settings)
        self.assertEqual(note.content, "milk and eggs")
        self.assertEqual(note.role, "document")
        self.assertEqual(note.kind, "apple-notes:note")
        self.assertEqual(note.title, "Shopping")
        self.assertEqual(note.metadata["folder"], "Notes")

    def test_dates_use_the_apple_epoch_in_milliseconds(self) -> None:
        (note,) = notes(self.settings)
        self.assertEqual(note.created_at, int((APPLE.timestamp() + 10) * 1000))
        self.assertEqual(note.updated_at, int((APPLE.timestamp() + 20) * 1000))

    def test_an_unknown_blob_yields_empty_text_rather_than_guessing(self) -> None:
        self.assertEqual(note_text(gzip.compress(b"\x08\x01")), "")
        self.assertEqual(note_text(b"not gzip"), "")

    def test_the_source_key_is_device_scoped(self) -> None:
        self.assertEqual(self.settings.source_id, "apple-notes:james")


if __name__ == "__main__":
    unittest.main()
