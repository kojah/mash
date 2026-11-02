"""iMessage reader over a synthetic chat.db copy."""

import sqlite3
import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path

from connectors.imessage_sqlite import SqliteIMessage

SCHEMA = """
CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, guid TEXT UNIQUE, display_name TEXT);
CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT UNIQUE);
CREATE TABLE chat_handle_join (chat_id INTEGER, handle_id INTEGER);
CREATE TABLE message (ROWID INTEGER PRIMARY KEY, guid TEXT UNIQUE, text TEXT,
  date INTEGER, is_from_me INTEGER, handle_id INTEGER, associated_message_guid TEXT);
CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);
"""

APPLE = datetime(2001, 1, 1, tzinfo=UTC)


def nanos(moment: datetime) -> int:
    return int((moment - APPLE).total_seconds() * 1_000_000_000)


class ReaderTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        path = Path(temporary.name) / "chat.db"
        con = sqlite3.connect(path)
        con.executescript(SCHEMA)
        con.execute("INSERT INTO chat VALUES (1, 'iMessage;-;+1555', NULL)")
        con.execute("INSERT INTO handle VALUES (1, '+1555')")
        con.execute("INSERT INTO chat_handle_join VALUES (1, 1)")
        first = datetime(2026, 10, 1, 12, 0, tzinfo=UTC)
        second = datetime(2026, 10, 2, 12, 0, tzinfo=UTC)
        con.execute(
            "INSERT INTO message VALUES (1, 'm1', 'hello', ?, 0, 1, NULL)", (nanos(first),)
        )
        con.execute(
            "INSERT INTO message VALUES (2, 'm2', 'hi back', ?, 1, NULL, 'm1')",
            (nanos(second),),
        )
        con.execute("INSERT INTO chat_message_join VALUES (1, 1), (1, 2)")
        con.commit()
        con.close()
        self.reader = SqliteIMessage(path)
        self.addCleanup(self.reader.close)

    async def test_chats_expose_participants(self) -> None:
        (conversation,) = await self.reader.chats(["iMessage;-;+1555"])
        self.assertEqual(conversation.participants[0].id, "+1555")

    async def test_history_is_newest_first_with_senders_and_replies(self) -> None:
        conversation = await self.reader.chat("iMessage;-;+1555")
        posts = [post async for post in self.reader.history(conversation, since=None)]
        self.assertEqual([post.id for post in posts], ["m2", "m1"])
        self.assertEqual(posts[0].sender.id, "me")
        self.assertEqual(posts[1].sender.id, "+1555")
        self.assertEqual(posts[0].reply_to, "m1")
        self.assertEqual(posts[1].created_at, datetime(2026, 10, 1, 12, 0, tzinfo=UTC))

    async def test_the_window_skips_older_messages(self) -> None:
        conversation = await self.reader.chat("iMessage;-;+1555")
        cutoff = datetime(2026, 10, 2, tzinfo=UTC)
        posts = [post async for post in self.reader.history(conversation, since=cutoff)]
        self.assertEqual([post.id for post in posts], ["m2"])


if __name__ == "__main__":
    unittest.main()
