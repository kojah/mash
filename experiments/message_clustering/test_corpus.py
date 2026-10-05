import math
import sqlite3
import tempfile
import unittest
from pathlib import Path

import orjson

from corpus import (
    Message,
    Provider,
    Split,
    Thread,
    leakage_components,
    read_threads,
    select_threads,
)


def fixture_thread(identity: str, provider: Provider, path: Path, count: int) -> Thread:
    messages = tuple(
        Message(
            id=f"{identity}-{ordinal}",
            ordinal=ordinal,
            role="user",
            projection=f"{identity} substantive projection number {ordinal}",
            context_parent=None,
            dimensions=2,
            norm=math.sqrt(13),
            vector_hex="0203",
        )
        for ordinal in range(count)
    )
    return Thread(
        id=identity,
        provider=provider,
        session=identity,
        source_path=path,
        title=None,
        canonical_messages=count + 2,
        searchable_messages=count + 1,
        generation="fixture",
        messages=messages,
    )


class CorpusContracts(unittest.TestCase):
    def test_sqlite_read_preserves_source_and_reports_unembedded_messages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "fixture.sqlite3"
            with sqlite3.connect(database) as db:
                db.executescript("""
                    CREATE TABLE gold_manifest(manifest TEXT);
                    INSERT INTO gold_manifest VALUES
                        ('{"build":{"embedding_text":"dialogue"}}');
                    CREATE TABLE conversations(id TEXT,provider TEXT,
                        source_session TEXT,source_path TEXT,title TEXT);
                    INSERT INTO conversations VALUES
                        ('thread','codex','session','unused',NULL);
                    CREATE TABLE messages(id TEXT,conversation_id TEXT,
                        ordinal INTEGER,role TEXT,content TEXT,search_projection TEXT,
                        context_parent INTEGER);
                    INSERT INTO messages VALUES
                        ('a','thread',0,'user','fallback',NULL,-1),
                        ('b','thread',1,'assistant','canonical','projection',0),
                        ('c','thread',2,'tool','',NULL,NULL);
                    CREATE TABLE message_embeddings(message_id TEXT,generation TEXT,
                        dimensions INTEGER,norm REAL,vector BLOB);
                """)
                db.executemany(
                    "INSERT INTO message_embeddings VALUES (?,'fixture',2,?,?)",
                    [
                        (identity, math.sqrt(13), bytes([2, 3]))
                        for identity in ("a", "b")
                    ],
                )
            before = database.read_bytes()
            _, threads = read_threads(database)
            self.assertEqual(database.read_bytes(), before)
            self.assertEqual(threads[0].canonical_messages, 3)
            self.assertEqual(threads[0].searchable_messages, 2)
            self.assertEqual(len(threads[0].messages), 2)
            self.assertEqual(
                [message.projection for message in threads[0].messages],
                ["fallback", "projection"],
            )
            self.assertEqual(
                [message.context_parent for message in threads[0].messages], [-1, 0]
            )
            with sqlite3.connect(database) as db:
                db.execute(
                    "UPDATE message_embeddings SET generation='other' "
                    "WHERE message_id='b'"
                )
            with self.assertRaisesRegex(ValueError, "mixes embedding generations"):
                read_threads(database)

    def test_shared_missing_parent_and_claude_uuid_join(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            threads: list[Thread] = []
            for identity, provider, lineage in (
                ("a", Provider.CODEX, "absent-parent"),
                ("b", Provider.CODEX, "absent-parent"),
                ("c", Provider.CLAUDE, "shared-message"),
                ("d", Provider.CLAUDE, "shared-message"),
            ):
                path = root / identity
                record = (
                    {
                        "type": "session_meta",
                        "payload": {"id": identity, "parent_thread_id": lineage},
                    }
                    if provider == Provider.CODEX
                    else {"uuid": identity, "parentUuid": lineage}
                )
                path.write_bytes(orjson.dumps(record) + b"\n")
                threads.append(fixture_thread(identity, provider, path, 8))
            components, checked = leakage_components(tuple(threads))
            self.assertEqual(
                components, (("a", "b"), ("a", "b"), ("c", "d"), ("c", "d"))
            )
            self.assertEqual(checked, 4)

    def test_copied_projections_join_without_shared_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = fixture_thread("a", Provider.CODEX, root / "a", 8)
            second = fixture_thread("b", Provider.CODEX, root / "b", 8)
            second = second.model_copy(
                update={"messages": first.messages[:4] + second.messages[4:]}
            )
            for thread in (first, second):
                thread.source_path.write_bytes(
                    orjson.dumps({"type": "session_meta", "payload": {"id": thread.id}})
                )
            components, _ = leakage_components((first, second))
            self.assertEqual(components, (("a", "b"), ("a", "b")))

    def test_balanced_deterministic_split_and_insufficient_components(self) -> None:
        threads = tuple(
            fixture_thread(
                f"{provider}-{count}-{index}", provider, Path("unused"), count
            )
            for provider in Provider
            for count in (8, 25, 1001)
            for index in range(4)
        )
        components = tuple((thread.id,) for thread in threads)
        selected = select_threads(threads, components)
        self.assertEqual(len(selected), 24)
        self.assertEqual(
            sum(entry.split == Split.DEVELOPMENT for entry in selected), 12
        )
        self.assertEqual(selected, select_threads(threads, components))
        by_split = {
            split: {entry.component for entry in selected if entry.split == split}
            for split in Split
        }
        self.assertFalse(by_split[Split.DEVELOPMENT] & by_split[Split.HELD_OUT])
        with self.assertRaises(ValueError):
            select_threads(threads, (("one-common-component",),) * len(threads))


if __name__ == "__main__":
    unittest.main()
