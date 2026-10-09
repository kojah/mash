"""Idealized Discord connector; channels, including thread channels, are chats."""

from collections.abc import AsyncIterator, Sequence
from dataclasses import dataclass
from datetime import datetime
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
class Channel:
    id: str
    name: str
    guild_id: str | None
    parent_channel_id: str | None
    url: str


@dataclass(frozen=True)
class Post:
    id: str
    channel_id: str
    text: str
    sender: Sender
    created_at: datetime
    updated_at: datetime | None
    url: str
    files: Sequence[File]
    reply_channel_id: str | None = None
    reply_message_id: str | None = None


@dataclass(frozen=True)
class Event:
    operation: Operation
    channel_id: str
    message_id: str | None = None


class Discord(Protocol):
    """Adapter owns REST paging, Gateway resume, intents, and partial-update fetches."""

    async def channels(self, ids: Sequence[str]) -> Sequence[Channel]: ...
    async def channel(self, id: str) -> Channel: ...
    def history(
        self, channel: Channel, *, since: datetime | None
    ) -> AsyncIterator[Post]: ...
    async def message(self, channel_id: str, id: str) -> Post: ...
    def events(self, channels: Sequence[str]) -> AsyncIterator[Delivery[Event]]: ...


def chat(channel: Channel) -> Chat:
    return Chat(
        id=channel.id,
        title=channel.name,
        url=channel.url,
        metadata={
            "guild_id": channel.guild_id,
            "parent_channel_id": channel.parent_channel_id,
        },
    )


def message(post: Post) -> Message:
    same_chat = post.reply_channel_id == post.channel_id
    return Message(
        id=post.id,
        chat_id=post.channel_id,
        content=post.text,
        author=post.sender.to_author(),
        reply_to=post.reply_message_id if same_chat else None,
        created_at=milliseconds(post.created_at),
        updated_at=milliseconds(post.updated_at),
        url=post.url,
        attachments=[file.to_attachment() for file in post.files],
        metadata={
            "native_reference": {
                "channel_id": post.reply_channel_id,
                "message_id": post.reply_message_id,
            }
        },
    )


async def backfill(source: Discord, scope: Scope) -> AsyncIterator[Snapshot]:
    since = scope.cutoff()
    for channel in await source.channels(scope.ids):
        if channel.id not in scope.ids:
            continue
        messages = [
            message(post)
            async for post in source.history(channel, since=since)
            if since is None or post.created_at >= since
        ]
        yield Snapshot(
            chat=chat(channel), messages=messages, complete=True, created_since=since
        )


async def refresh(source: Discord, event: Event, scope: Scope) -> Changes:
    if event.channel_id not in scope.ids:
        return Changes.empty()
    match event.operation:
        case Operation.DELETE_CHAT:
            return Changes.delete_chats(event.channel_id)
        case Operation.REFRESH_CHAT:
            return Changes.upsert(chat(await source.channel(event.channel_id)))
        case Operation.DELETE_MESSAGE | Operation.REFRESH_MESSAGE:
            if event.message_id is None:
                raise ValueError("message events require a message ID")
            if event.operation == Operation.DELETE_MESSAGE:
                return Changes.delete_messages(event.message_id)
            post = await source.message(event.channel_id, event.message_id)
            if not scope.includes(post.channel_id, post.created_at):
                return Changes.empty()
            # Gateway updates omit fields. Always publish the full current record.
            return Changes.upsert(
                chat(await source.channel(post.channel_id)), message(post)
            )


async def run(
    source: Discord,
    *,
    account_id: str,
    scope: Scope,
    destination: Destination,
    state_path: Path,
) -> None:
    async with Connector(
        source_id=f"discord:{account_id}",
        destination=destination,
        state_path=state_path,
    ) as connector:
        await connector.run(
            backfill=backfill(source, scope),
            events=source.events(scope.ids),
            refresh=lambda event: refresh(source, event, scope),
            debounce=Debounce(
                quiet_seconds=scope.quiet_seconds,
                max_wait_seconds=scope.max_wait_seconds,
            ),
        )
