"""Protect provenance, duplicate handling and fail-closed experiment inputs."""
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

import numpy as np

from data import decode_vector, load


class FrozenInputFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.db = self.root / "input.sqlite3"
        self.partition = self.root / "partition.json"
        self.partition.write_text(json.dumps({"generation": "frozen", "partition": {
            "source_ids": {"a": [0] * 32, "alias": [0] * 32, "b": [1] * 32}}}))
        vector = np.ones(384, dtype=np.int8).tobytes()
        with sqlite3.connect(self.db) as db:
            db.executescript("""
                CREATE TABLE conversations(id TEXT PRIMARY KEY, provider TEXT);
                CREATE TABLE messages(id TEXT PRIMARY KEY, conversation_id TEXT,
                    role TEXT, ordinal INTEGER, content TEXT, search_projection TEXT);
                CREATE TABLE message_embeddings(message_id TEXT, vector BLOB, generation TEXT,
                    dimensions INTEGER, norm REAL);
            """)
            for thread in ("a", "alias", "b"):
                db.execute("INSERT INTO conversations VALUES (?, 'codex')", (thread,))
                for ordinal in range(2):
                    message = f"{thread}-{ordinal}"
                    db.execute("INSERT INTO messages VALUES (?,?,'user',?,'source','topic input')",
                               (message, thread, ordinal))
                    db.execute("INSERT INTO message_embeddings VALUES (?,?,'frozen',384,?)",
                               (message, vector, float(np.sqrt(np.float32(384)))))


class FrozenInputTests(FrozenInputFixture):
    def test_preserves_groups_deduplicates_and_does_not_write(self):
        before = self.db.read_bytes()
        first = load(self.db, self.partition)
        second = load(self.db, self.partition)
        self.assertEqual(first.fingerprint, second.fingerprint)
        self.assertEqual(before, self.db.read_bytes())
        self.assertEqual([t.id for t in first.threads], ["a", "b"])
        self.assertEqual([t.group[0] % 5 == 0 for t in first.threads], [True, False])
        self.assertEqual((first.stored_user_vectors, first.duplicate_messages, first.duplicate_threads), (6, 3, 1))

    def test_incompatible_generation_fails(self):
        with sqlite3.connect(self.db) as db:
            db.execute("UPDATE message_embeddings SET generation='other' WHERE message_id='a-0'")
        with self.assertRaisesRegex(ValueError, "generation mismatch"):
            load(self.db, self.partition)

    def test_invalid_dimension_fails(self):
        with sqlite3.connect(self.db) as db:
            db.execute("UPDATE message_embeddings SET vector=x'01' WHERE message_id='a-0'")
        with self.assertRaisesRegex(ValueError, "dimension"):
            load(self.db, self.partition)

    def test_missing_partition_fails(self):
        report = json.loads(self.partition.read_text())
        del report["partition"]["source_ids"]["b"]
        self.partition.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, "missing"):
            load(self.db, self.partition)

    def test_corrupt_stored_norm_fails(self):
        with sqlite3.connect(self.db) as db:
            db.execute("UPDATE message_embeddings SET norm=0 WHERE message_id='a-0'")
        with self.assertRaisesRegex(ValueError, "stored norm"):
            load(self.db, self.partition)

    def test_signed_byte_encoding_preserves_negative_direction(self):
        values = np.zeros(384, dtype=np.int8)
        values[:2] = (127, -127)
        vector = decode_vector(values.tobytes(), 384, float(np.sqrt(np.float32(2 * 127 * 127))))
        self.assertAlmostEqual(vector[0], 1 / np.sqrt(2))
        self.assertAlmostEqual(vector[1], -1 / np.sqrt(2))


if __name__ == "__main__":
    unittest.main()
