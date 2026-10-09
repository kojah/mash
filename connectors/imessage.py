"""Idealized iMessage connector; the SQLite reader implements the source port."""

from collections.abc import AsyncIterator, Sequence
from dataclasses import dataclass
from datetime import datetime
from math import isfinite
from pathlib import Path
from typing import Protocol

from mash_sdk import (
    Changes,
    Chat,
    Connector,
    Debounce,
    Delivery,
    Destination,
    Message,
    Snapshot,
)

from .common import File, Operation, Sender, milliseconds
from .scope import Scope


@dataclass(frozen=True)
class Settings:
    database_path: Path
    account_scope: str
    scope: Scope
    poll_seconds: float = 10

    def __post_init__(self) -> None:
        if not isfinite(self.poll_seconds) or self.poll_seconds <= 0:
            raise ValueError("poll_seconds must be finite and positive")


@dataclass(frozen=True)
class Conversation:
    id: str
    title: str | None
    participants: Sequence[Sender]


@dataclass(frozen=True)
class Post:
    id: str
    chat_id: str
    content: str
    sender: Sender | None
    created_at: datetime | None
    updated_at: datetime | None
    files: Sequence[File]
    reply_to: str | None = None


@dataclass(frozen=True)
class Event:
    operation: Operation
    chat_id: str
    message_id: str | None = None


class IMessage(Protocol):
    """Read-only, snapshot-based reader with versioned native decoding.

    The reader owns chat/message joins, sender handles (including outgoing
    messages), Apple timestamps, rich-text bodies, reactions and attachment files.
    IDs should derive from stable native GUIDs, not transient row numbers.
    """

    async def chats(self, ids: Sequence[str]) -> Sequence[Conversation]: ...
    async def chat(self, id: str) -> Conversation: ...
    def history(
        self, chat: Conversation, *, since: datetime | None
    ) -> AsyncIterator[Post]: ...
    async def message(self, chat_id: str, id: str) -> Post: ...
    def events(self, ids: Sequence[str]) -> AsyncIterator[Delivery[Event]]: ...


def chat(conversation: Conversation) -> Chat:
    return Chat(
        id=conversation.id,
        title=conversation.title,
        metadata={
            "participants": [
                {"id": participant.id, "name": participant.name}
                for participant in conversation.participants
            ],
        },
    )


def message(post: Post) -> Message:
    return Message(
        id=f"{post.chat_id}:{post.id}",
        chat_id=post.chat_id,
        content=post.content,
        author=post.sender.to_author() if post.sender else None,
        reply_to=f"{post.chat_id}:{post.reply_to}" if post.reply_to else None,
        created_at=milliseconds(post.created_at),
        updated_at=milliseconds(post.updated_at),
        attachments=[file.to_attachment() for file in post.files],
        metadata={"native_guid": post.id},
    )


async def backfill(source: IMessage, scope: Scope) -> AsyncIterator[Snapshot]:
    since = scope.cutoff()
    for conversation in await source.chats(scope.ids):
        if conversation.id not in scope.ids:
            continue
        posts = [
            message(post)
            async for post in source.history(conversation, since=since)
            if since is None or post.created_at is not None and post.created_at >= since
        ]
        # A local database can be pruned independently of the source conversation.
        # Its absence alone must not retract already imported history.
        yield Snapshot(
            chat=chat(conversation), messages=posts, complete=False, created_since=since
        )


async def refresh(source: IMessage, event: Event, scope: Scope) -> Changes:
    if event.chat_id not in scope.ids:
        return Changes.empty()
    match event.operation:
        case Operation.DELETE_CHAT:
            return Changes.delete_chats(event.chat_id)
        case Operation.REFRESH_CHAT:
            return Changes.upsert(chat(await source.chat(event.chat_id)))
        case Operation.DELETE_MESSAGE | Operation.REFRESH_MESSAGE:
            if event.message_id is None:
                raise ValueError("message events require a stable native message ID")
            if event.operation == Operation.DELETE_MESSAGE:
                # The adapter must provide affirmative deletion evidence, not missing rows.
                return Changes.delete_messages(f"{event.chat_id}:{event.message_id}")
            post = await source.message(event.chat_id, event.message_id)
            if not scope.includes(post.chat_id, post.created_at):
                return Changes.empty()
            return Changes.upsert(chat(await source.chat(post.chat_id)), message(post))


async def run(
    source: IMessage,
    settings: Settings,
    *,
    destinations: Sequence[Destination],
    state_path: Path,
) -> None:
    async with Connector(
        source_id=f"imessage:{settings.account_scope}",
        destinations=destinations,
        state_path=state_path,
    ) as connector:
        await connector.run(
            backfill=backfill(source, settings.scope),
            events=source.events(settings.scope.ids),
            event_type=Event,
            refresh=lambda event: refresh(source, event, settings.scope),
            debounce=Debounce(
                quiet_seconds=settings.scope.quiet_seconds,
                max_wait_seconds=settings.scope.max_wait_seconds,
            ),
        )
