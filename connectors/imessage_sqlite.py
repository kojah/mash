"""Read-only iMessage history over a copied chat.db.

The reader implements the `IMessage` protocol against Apple's schema: `chat`,
`handle` and `message`, with Apple's 2001-01-01 epoch. Bodies come from `text`
or, when Apple stores them only as attributed body, are left empty rather than
guessed: a missing body is not a deletion and must not invent text. Callers copy
the database (plus -wal/-shm) before opening it; nothing here writes.
"""

import sqlite3
from collections.abc import AsyncIterator, Sequence
from datetime import UTC, datetime, timedelta
from pathlib import Path

from .blueprints.imessage import Conversation, Post
from .common import Sender

APPLE_EPOCH = datetime(2001, 1, 1, tzinfo=UTC)
NANOSECONDS = 1_000_000_000


class SqliteIMessage:
    """A snapshot reader; open one per copy of the database."""

    def __init__(self, path: Path) -> None:
        self.connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        self.connection.row_factory = sqlite3.Row

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
            content=row["text"] or "",
            sender=sender,
            created_at=created,
            updated_at=created,
            files=[],
            reply_to=row["associated_message_guid"] or None,
        )

    def history(self, chat: Conversation, *, since: datetime | None) -> AsyncIterator[Post]:
        rows = self.connection.execute(
            "SELECT m.guid, m.text, m.date, m.is_from_me, m.associated_message_guid,"
            " h.id AS handle FROM message m"
            " JOIN chat_message_join j ON j.message_id = m.ROWID"
            " LEFT JOIN handle h ON h.ROWID = m.handle_id"
            " WHERE j.chat_id = (SELECT ROWID FROM chat WHERE guid = ?)"
            " ORDER BY m.date DESC",
            (chat.id,),
        )
        found = [self._post(row, chat.id) for row in rows]
        if since is not None:
            found = [p for p in found if p.created_at is not None and p.created_at >= since]
        return _iterator(found)

    async def message(self, chat_id: str, id: str) -> Post:
        row = self.connection.execute(
            "SELECT m.guid, m.text, m.date, m.is_from_me, m.associated_message_guid,"
            " h.id AS handle FROM message m"
            " LEFT JOIN handle h ON h.ROWID = m.handle_id WHERE m.guid = ?",
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


def _moment(value: int | None) -> datetime | None:
    if value is None:
        return None
    # Apple stores nanoseconds since 2001-01-01; some rows use seconds.
    seconds = value / NANOSECONDS if value > 10**15 else value
    return APPLE_EPOCH + timedelta(seconds=seconds)
