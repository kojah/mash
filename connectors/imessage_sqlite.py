"""Read-only iMessage history over a copied chat.db.

The reader implements the `IMessage` protocol against Apple's schema: `chat`,
`handle` and `message`, with Apple's 2001-01-01 epoch. Bodies come from `text`
or, as on current macOS where most messages leave `text` empty, from the plain
string inside the `attributedBody` NSAttributedString archive. A body neither
holds stays empty rather than guessed: a missing body is not a deletion. Callers copy
the database (plus -wal/-shm) before opening it; nothing here writes.
"""

import sqlite3
from collections.abc import AsyncIterator, Sequence
from datetime import UTC, datetime, timedelta
from pathlib import Path

from .blueprints.imessage import Conversation, Post
from .common import File, Sender

APPLE_EPOCH = datetime(2001, 1, 1, tzinfo=UTC)
NANOSECONDS = 1_000_000_000


class SqliteIMessage:
    """A snapshot reader; open one per copy of the database."""

    def __init__(self, path: Path) -> None:
        self.connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        self.connection.row_factory = sqlite3.Row
        # Older macOS databases lack some columns; read what this one has.
        columns = {row[1] for row in self.connection.execute("PRAGMA table_info(message)")}
        tables = {
            row[0]
            for row in self.connection.execute("SELECT name FROM sqlite_master WHERE type='table'")
        }
        optional = lambda name: f"m.{name}" if name in columns else f"NULL AS {name}"
        self.select = (
            "SELECT m.ROWID, m.guid, m.text, m.attributedBody, m.date, m.is_from_me,"
            f" {optional('thread_originator_guid')}, h.id AS handle FROM message m"
        )
        # Reactions (tapbacks) and system events (renames, joins) are not
        # messages; reactions point at another message and carry no text.
        self.conversational = " AND ".join(
            filter(
                None,
                [
                    "COALESCE(m.associated_message_type, 0) NOT BETWEEN 2000 AND 3999"
                    if "associated_message_type" in columns
                    else "",
                    "COALESCE(m.item_type, 0) = 0" if "item_type" in columns else "",
                ],
            )
        ) or "1"
        self.attachments = {"attachment", "message_attachment_join"} <= tables

    def close(self) -> None:
        self.connection.close()

    def _chat(self, row: sqlite3.Row) -> Conversation:
        participants = [
            Sender(id=row["handle"], name=None, kind="human")
            for row in self.connection.execute(
                "SELECT DISTINCT h.id AS handle FROM chat_handle_join j"
                " JOIN handle h ON h.ROWID = j.handle_id WHERE j.chat_id = ?",
                (row["ROWID"],),
            )
        ]
        return Conversation(
            id=row["guid"], title=row["display_name"] or None, participants=participants
        )

    async def chats(self, ids: Sequence[str]) -> Sequence[Conversation]:
        found = []
        for guid in ids:
            row = self.connection.execute(
                "SELECT ROWID, guid, display_name FROM chat WHERE guid = ?", (guid,)
            ).fetchone()
            if row is not None:
                found.append(self._chat(row))
        return found

    async def chat(self, id: str) -> Conversation:
        rows = await self.chats([id])
        if not rows:
            raise LookupError(f"no chat {id}")
        return rows[0]

    def _files(self, message: int) -> list[File]:
        """Attachment metadata; the files stay where Messages keeps them."""
        if not self.attachments:
            return []
        rows = self.connection.execute(
            "SELECT a.guid, a.filename, a.transfer_name, a.mime_type, a.total_bytes"
            " FROM message_attachment_join j JOIN attachment a ON a.ROWID = j.attachment_id"
            " WHERE j.message_id = ? ORDER BY a.ROWID",
            (message,),
        )
        found = []
        for row in rows:
            path = Path(row["filename"]).expanduser() if row["filename"] else None
            found.append(
                File(
                    id=row["guid"],
                    name=row["transfer_name"] or (path.name if path else row["guid"]),
                    media_type=row["mime_type"] or "application/octet-stream",
                    url=path.as_uri() if path and path.is_absolute() else "",
                    size=row["total_bytes"] or None,
                )
            )
        return found

    def _post(self, row: sqlite3.Row, chat_id: str) -> Post:
        sender = None
        if row["handle"] is not None and not row["is_from_me"]:
            sender = Sender(id=row["handle"], name=None, kind="human")
        elif row["is_from_me"]:
            sender = Sender(id="me", name=None, kind="human")
        created = _moment(row["date"])
        return Post(
            id=row["guid"],
            chat_id=chat_id,
            content=row["text"] or attributed_text(row["attributedBody"]) or "",
            sender=sender,
            created_at=created,
            updated_at=created,
            files=self._files(row["ROWID"]),
            # An inline reply names the message that started its thread.
            reply_to=row["thread_originator_guid"] or None,
        )

    def history(self, chat: Conversation, *, since: datetime | None) -> AsyncIterator[Post]:
        rows = self.connection.execute(
            f"{self.select}"
            " JOIN chat_message_join j ON j.message_id = m.ROWID"
            " LEFT JOIN handle h ON h.ROWID = m.handle_id"
            " WHERE j.chat_id = (SELECT ROWID FROM chat WHERE guid = ?)"
            f" AND {self.conversational}"
            " ORDER BY m.date DESC",
            (chat.id,),
        )
        found = [self._post(row, chat.id) for row in rows]
        if since is not None:
            found = [p for p in found if p.created_at is not None and p.created_at >= since]
        return _iterator(found)

    async def message(self, chat_id: str, id: str) -> Post:
        row = self.connection.execute(
            f"{self.select} LEFT JOIN handle h ON h.ROWID = m.handle_id"
            f" WHERE m.guid = ? AND {self.conversational}",
            (id,),
        ).fetchone()
        if row is None:
            raise LookupError(f"no message {id}")
        return self._post(row, chat_id)

    def events(self, ids: Sequence[str]) -> AsyncIterator:
        return _iterator([])


async def _iterator(items):
    for item in items:
        yield item


def attributed_text(blob: bytes | None) -> str | None:
    """The string of an NSAttributedString typedstream, or None if not found.

    The archive names the class `NSString`, then after a `+` type marker holds
    the UTF-8 bytes with a length: one byte, or 0x81 and two little-endian
    bytes, or 0x82 and four.
    """
    if not blob:
        return None
    start = blob.find(b"NSString")
    if start < 0:
        return None
    marker = blob.find(b"+", start + len(b"NSString"))
    if marker < 0 or marker + 1 >= len(blob):
        return None
    offset = marker + 1
    length = blob[offset]
    offset += 1
    if length == 0x81:
        length = int.from_bytes(blob[offset : offset + 2], "little")
        offset += 2
    elif length == 0x82:
        length = int.from_bytes(blob[offset : offset + 4], "little")
        offset += 4
    text = blob[offset : offset + length]
    if len(text) != length:
        return None
    return text.decode("utf-8", "replace")


def _moment(value: int | None) -> datetime | None:
    if value is None:
        return None
    # Apple stores nanoseconds since 2001-01-01; some rows use seconds.
    seconds = value / NANOSECONDS if value > 10**15 else value
    return APPLE_EPOCH + timedelta(seconds=seconds)
