"""Benchmark witnesses read canonical packed silver without a gold database."""

from contextlib import closing
import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import tomllib
import unittest

SPEC = importlib.util.spec_from_file_location(
    "benchmark_views", Path(__file__).with_name("benchmark-views.py")
)
if SPEC is None or SPEC.loader is None:
    raise ImportError("benchmark-views.py could not be loaded")
COST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COST)


class CanonicalWitnessTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.database = Path(self.directory.name) / "silver.sqlite3"
        schema = Path(__file__).resolve().parents[1] / "app/storage/silver.sql"
        with closing(sqlite3.connect(self.database)) as connection:
            # Only schema creation knows this function. The witness connection
            # must read stored bytes, never depend on a Python silver codec.
            connection.create_function("silver_text", 1, lambda _: "canonical text", deterministic=True)
            connection.executescript(schema.read_text())
            connection.execute(
                "INSERT INTO chats(id,provider) VALUES('thread','codex')"
            )
            connection.execute(
                "INSERT INTO messages(id,chat_id,ordinal,role,content_packed) "
                "VALUES('message','thread',0,'user',?)",
                (b"opaque packed canonical bytes",),
            )
            connection.commit()

    def test_digest_tracks_packed_silver_but_not_publication_bookkeeping(self) -> None:
        original = COST.silver_digest(self.database)
        with closing(sqlite3.connect(self.database)) as connection:
            connection.execute("UPDATE shared_publication SET revision=revision+1")
            connection.commit()
        self.assertEqual(COST.silver_digest(self.database), original)
        with closing(sqlite3.connect(self.database)) as connection:
            connection.create_function("silver_text", 1, lambda _: "canonical text", deterministic=True)
            connection.execute("UPDATE messages SET content_packed=?", (b"changed bytes",))
            connection.commit()
        self.assertNotEqual(COST.silver_digest(self.database), original)

    def test_storage_reports_silver_without_opening_gold(self) -> None:
        report = COST.storage(self.database)
        self.assertEqual(report["scope"], "sqlite-silver-only")
        self.assertEqual(report["counts"]["chats"], 1)
        self.assertEqual(report["counts"]["messages"], 1)
        self.assertEqual(report["counts"]["turns"], 0)
        self.assertFalse(self.database.with_suffix(".views.sqlite3").exists())

    def test_explicit_backend_configuration_is_strict_toml(self) -> None:
        configuration = COST.qdrant_configuration("http://127.0.0.1:6333", "http://[::1]:6334")
        self.assertEqual(tomllib.loads(configuration)["qdrant"], {
            "url": "http://127.0.0.1:6333", "grpc_url": "http://[::1]:6334"
        })
        for invalid in ("http://example.com:6333", "http://localhost", "http://user@localhost:6333"):
            with self.subTest(endpoint=invalid), self.assertRaises(ValueError):
                COST.qdrant_configuration(invalid, "http://127.0.0.1:6334")


if __name__ == "__main__":
    unittest.main()
