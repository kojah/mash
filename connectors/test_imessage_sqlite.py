"""iMessage reader over a synthetic chat.db copy."""

import sqlite3
import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path

from connectors.imessage_sqlite import SqliteIMessage, attributed_text

SCHEMA = """
CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, guid TEXT UNIQUE, display_name TEXT);
CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT UNIQUE);
CREATE TABLE chat_handle_join (chat_id INTEGER, handle_id INTEGER);
CREATE TABLE message (ROWID INTEGER PRIMARY KEY, guid TEXT UNIQUE, text TEXT,
  date INTEGER, is_from_me INTEGER, handle_id INTEGER, thread_originator_guid TEXT,
  attributedBody BLOB, associated_message_guid TEXT, associated_message_type INTEGER,
  item_type INTEGER);
CREATE TABLE attachment (ROWID INTEGER PRIMARY KEY, guid TEXT, filename TEXT,
  transfer_name TEXT, mime_type TEXT, total_bytes INTEGER);
CREATE TABLE message_attachment_join (message_id INTEGER, attachment_id INTEGER);
CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);
"""

APPLE = datetime(2001, 1, 1, tzinfo=UTC)
# The header current macOS writes before an NSAttributedString's string.
TYPEDSTREAM = (
    b"\x04\x0bstreamtyped\x81\xe8\x03\x84\x01@\x84\x84\x84\x12NSAttributedString"
    b"\x00\x84\x84\x08NSObject\x00\x85\x92\x84\x84\x84\x08NSString\x01\x94\x84\x01+"
)


def archived(text: str) -> bytes:
    """An attributedBody holding `text`, with typedstream's length prefix."""
    data = text.encode()
    if len(data) < 0x80:
        size = bytes([len(data)])
    else:
        size = b"\x81" + len(data).to_bytes(2, "little")
    return TYPEDSTREAM + size + data + b"\x86\x84\x02iI\x01"


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
            "INSERT INTO message VALUES (1, 'm1', 'hello', ?, 0, 1, NULL, NULL, NULL, 0, 0)",
            (nanos(first),),
        )
        con.execute(
            "INSERT INTO message VALUES (2, 'm2', 'hi back', ?, 1, NULL, 'm1', NULL,"
            " NULL, 0, 0)",
            (nanos(second),),
        )
        con.execute(
            "INSERT INTO message VALUES (3, 'm3', NULL, ?, 0, 1, NULL, ?, NULL, 0, 0)",
            (nanos(second) + 1, archived("only in the archive")),
        )
        # A tapback on m1 and a group rename: neither is a message.
        con.execute(
            "INSERT INTO message VALUES (4, 'r1', NULL, ?, 1, NULL, NULL, ?, 'p:0/m1', 2000, 0)",
            (nanos(second) + 2, archived("Loved “hello”")),
        )
        con.execute(
            "INSERT INTO message VALUES (5, 's1', NULL, ?, 0, NULL, NULL, NULL, NULL, 0, 2)",
            (nanos(second) + 3,),
        )
        con.execute(
            "INSERT INTO attachment VALUES (1, 'a1',"
            " '/Users/me/Library/Messages/Attachments/ab/photo.heic', 'IMG_1.heic',"
            " 'image/heic', 2048)"
        )
        con.execute("INSERT INTO message_attachment_join VALUES (1, 1)")
        con.execute(
            "INSERT INTO chat_message_join VALUES (1, 1), (1, 2), (1, 3), (1, 4), (1, 5)"
        )
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
        self.assertEqual([post.id for post in posts], ["m3", "m2", "m1"])
        self.assertEqual(posts[0].content, "only in the archive")
        self.assertEqual(posts[1].sender.id, "me")
        self.assertEqual(posts[2].sender.id, "+1555")
        self.assertEqual(posts[1].reply_to, "m1")
        self.assertEqual(posts[2].created_at, datetime(2026, 10, 1, 12, 0, tzinfo=UTC))

    async def test_reactions_and_system_events_are_not_messages(self) -> None:
        conversation = await self.reader.chat("iMessage;-;+1555")
        posts = [post async for post in self.reader.history(conversation, since=None)]
        self.assertNotIn("r1", [post.id for post in posts])
        self.assertNotIn("s1", [post.id for post in posts])
        with self.assertRaises(LookupError):
            await self.reader.message(conversation.id, "r1")

    async def test_attachments_are_described_where_messages_keeps_them(self) -> None:
        post = await self.reader.message("iMessage;-;+1555", "m1")
        (file,) = post.files
        self.assertEqual((file.id, file.name, file.media_type, file.size),
                         ("a1", "IMG_1.heic", "image/heic", 2048))
        self.assertEqual(
            file.url, "file:///Users/me/Library/Messages/Attachments/ab/photo.heic"
        )

    async def test_the_window_skips_older_messages(self) -> None:
        conversation = await self.reader.chat("iMessage;-;+1555")
        cutoff = datetime(2026, 10, 2, tzinfo=UTC)
        posts = [post async for post in self.reader.history(conversation, since=cutoff)]
        self.assertEqual([post.id for post in posts], ["m3", "m2"])


class AttributedBodyTests(unittest.TestCase):
    def test_the_archived_string_is_read_when_text_is_empty(self) -> None:
        self.assertEqual(attributed_text(archived("on my way ✓")), "on my way ✓")
        long = "word " * 60
        self.assertEqual(attributed_text(archived(long)), long)

    def test_an_unrecognized_archive_yields_nothing(self) -> None:
        self.assertIsNone(attributed_text(None))
        self.assertIsNone(attributed_text(b"no string here"))
        self.assertIsNone(attributed_text(archived("cut short")[:-12]))


if __name__ == "__main__":
    unittest.main()
