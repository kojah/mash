"""Verify research captures include committed WAL data and preserve originals."""

import hashlib
import sqlite3
import tempfile
import unittest
from pathlib import Path

from snapshot_node import Snapshot, capture


class SnapshotTests(unittest.TestCase):
    def test_live_wal_capture_is_private_and_source_preserving(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            native = root / "native.jsonl"
            native.write_bytes(b'{"uuid":"native-source"}\n')
            database = root / "original.sqlite3"
            with sqlite3.connect(database) as db:
                db.execute("PRAGMA journal_mode=WAL")
                db.execute(
                    "CREATE TABLE conversations(id TEXT,source_path TEXT,provider TEXT)"
                )
                db.execute(
                    "INSERT INTO conversations VALUES(?,?,?)",
                    ("history", str(native), "claude-code"),
                )
                db.execute(
                    "INSERT INTO conversations VALUES(?,?,?)",
                    ("missing", str(root / "absent"), "codex"),
                )
                db.commit()
                result = capture(database, root / "capture", "test-node")
                self.assertEqual(result.missing_sources, 1)
                self.assertEqual(result.sources[0].original_path, native)
                self.assertEqual(
                    result.sources[0].sha256,
                    hashlib.sha256(native.read_bytes()).hexdigest(),
                )
                with sqlite3.connect(root / "capture" / "mash.sqlite3") as copy:
                    self.assertEqual(
                        copy.execute("SELECT count(*) FROM conversations").fetchone()[
                            0
                        ],
                        2,
                    )
                self.assertEqual(
                    Snapshot.model_validate_json(
                        (root / "capture" / "snapshot.json").read_bytes()
                    ),
                    result,
                )
                self.assertEqual(native.read_bytes(), b'{"uuid":"native-source"}\n')
                self.assertEqual(
                    db.execute("SELECT count(*) FROM conversations").fetchone()[0], 2
                )
                for path in (root / "capture").rglob("*"):
                    if path.is_file():
                        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                with self.assertRaises(FileExistsError):
                    capture(database, root / "capture", "test-node")
