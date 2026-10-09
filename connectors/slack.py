"""Idealized Slack connector: channels are chats; replies are message links.

The source port owns Slack authentication, paging, rich-text rendering, and
event delivery. The desired Mash SDK owns durability, diffs, debounce and acks.
"""

from collections.abc import AsyncIterator, Sequence
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Protocol

from mash_sdk import (
    Changes,
    Connector,
    Debounce,
    Delivery,
    Destination,
    Object,
    Role,
    Snapshot,
)

from .common import File, Operation, Sender, milliseconds
from .scope import Scope


@dataclass(frozen=True)
class Channel:
    id: str
    name: str
    workspace_id: str
    workspace_name: str
    url: str
    private: bool
    archived: bool


@dataclass(frozen=True)
class Post:
    channel_id: str
    ts: str
    thread_ts: str | None
    text: str
    sender: Sender | None
    created_at: datetime
    updated_at: datetime | None
    url: str
    files: Sequence[File]


@dataclass(frozen=True)
class Event:
    operation: Operation
    channel_id: str
    message_ts: str | None = None


class Slack(Protocol):
    async def channels(self, ids: Sequence[str]) -> Sequence[Channel]: ...
    def history(
        self, channel: Channel, *, since: datetime | None
    ) -> AsyncIterator[Post]:
        """All pages, including replies; report access/paging failures as errors."""
        ...

    async def channel(self, id: str) -> Channel: ...
    async def message(self, channel_id: str, ts: str) -> Post: ...
    def events(self, channels: Sequence[str]) -> AsyncIterator[Delivery[Event]]: ...


def message_id(channel_id: str, ts: str) -> str:
    # Keep the original microsecond timestamp string: float conversion loses IDs.
    return f"{channel_id}:{ts}"


def chat(channel: Channel) -> Object:
    return Object(
        role=Role.container,
        kind="slack:channel",
        id=channel.id,
        title=channel.name,
        url=channel.url,
        metadata={
            "workspace": {"id": channel.workspace_id, "name": channel.workspace_name},
            "native_kind": "channel",
            "private": channel.private,
            "archived": channel.archived,
        },
    )


def message(post: Post) -> Object:
    parent = post.thread_ts if post.thread_ts != post.ts else None
    return Object(
        role=Role.entry,
        kind="slack:message",
        id=message_id(post.channel_id, post.ts),
        parent=post.channel_id,
        content=post.text,
        author=post.sender.to_author() if post.sender else None,
        reply_to=message_id(post.channel_id, parent) if parent else None,
        created_at=milliseconds(post.created_at),
        updated_at=milliseconds(post.updated_at),
        url=post.url,
        attachments=[file.to_attachment() for file in post.files],
        metadata={"native_ts": post.ts},
    )


async def backfill(source: Slack, scope: Scope) -> AsyncIterator[Snapshot]:
    since = scope.cutoff()
    for channel in await source.channels(scope.ids):
        if channel.id not in scope.ids:
            continue
        posts = [
            message(post)
            async for post in source.history(channel, since=since)
            if since is None or post.created_at >= since
        ]
        # A production adapter must expose scoped pages for very large channels.
        yield Snapshot(
            container=chat(channel), objects=posts, complete=True, created_since=since
        )


async def refresh(source: Slack, event: Event, scope: Scope) -> Changes:
    if event.channel_id not in scope.ids:
        return Changes.empty()
    match event.operation:
        case Operation.DELETE_CHAT:
            return Changes.delete(event.channel_id)
        case Operation.REFRESH_CHAT:
            return Changes.upsert(chat(await source.channel(event.channel_id)))
        case Operation.DELETE_MESSAGE | Operation.REFRESH_MESSAGE:
            if event.message_ts is None:
                raise ValueError(
                    "message events require the original message timestamp"
                )
            if event.operation == Operation.DELETE_MESSAGE:
                return Changes.delete(message_id(event.channel_id, event.message_ts))
            # Fetch current complete state; coalesced edits must not clear omitted fields.
            post = await source.message(event.channel_id, event.message_ts)
            if not scope.includes(post.channel_id, post.created_at):
                return Changes.empty()
            return Changes.upsert(
                chat(await source.channel(post.channel_id)), message(post)
            )


async def run(
    source: Slack,
    *,
    workspace_id: str,
    scope: Scope,
    destinations: Sequence[Destination],
    state_path: Path,
) -> None:
    async with Connector(
        source_id=f"slack:{workspace_id}",
        destinations=destinations,
        state_path=state_path,
    ) as connector:
        await connector.run(
            backfill=backfill(source, scope),
            events=source.events(scope.ids),
            event_type=Event,
            refresh=lambda event: refresh(source, event, scope),
            debounce=Debounce(
                quiet_seconds=scope.quiet_seconds,
                max_wait_seconds=scope.max_wait_seconds,
            ),
        )
