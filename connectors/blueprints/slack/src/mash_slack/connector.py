"""Polling and delivery; the Mash SDK owns retries, receipts and batch identities.

Each poll lists every channel's top-level messages from `thread_days` ago, or
from its cursor if that is older, so edits and new thread replies in that
lookback are seen. conversations.history only returns thread parents, whose
`latest_reply` changes when someone replies, so a thread is fetched with
conversations.replies only when that differs from the last poll. Replies to
parents older than the lookback, edits to replies in quiet threads, and
deletions are not seen: polling has no deletion events. The Events API or
Socket Mode would be needed for those.
"""

import asyncio
import hashlib
import os
from collections.abc import Callable
from dataclasses import asdict, dataclass
from datetime import UTC, datetime, timedelta
from decimal import Decimal
from pathlib import Path

import orjson
from aiohttp import ClientError
from mash_sdk import Changes, Connector, DeliveryError, Mode, Snapshot
from pydantic import BaseModel, TypeAdapter, ValidationError
from slack_sdk.errors import SlackApiError
from slack_sdk.web.async_client import AsyncWebClient

from .config import Settings
from .objects import (
    CONTENT,
    KINDS,
    MESSAGES,
    Channel,
    Message,
    People,
    User,
    Workspace,
    chat,
    mentions,
    message,
)

PAGE = 200


class WrongWorkspace(Exception):
    """The token belongs to a different team than `team_id`."""


class Cursor(BaseModel):
    """The newest listed message, and each listed thread's last reply."""

    newest: str
    threads: dict[str, str]


CURSORS = TypeAdapter(dict[str, Cursor])


@dataclass
class Result:
    source_id: str
    synced_channels: int
    listed_messages: int
    fetched_threads: int
    time_excluded_messages: int
    delivered_batches: int
    issues: list[str]

    def json(self) -> dict[str, object]:
        return asdict(self)


def cursor_path(settings: Settings) -> Path:
    # Changing destinations or the window restarts from the window start; the
    # SDK still skips what each destination already acknowledged.
    key = orjson.dumps(
        [
            [item.model_dump(mode="json") for item in settings.destinations],
            settings.history_days,
        ]
    )
    return settings.state_dir / f"cursors-{hashlib.sha256(key).hexdigest()[:16]}.json"


def load_cursors(path: Path) -> dict[str, Cursor]:
    return CURSORS.validate_json(path.read_bytes()) if path.exists() else {}


def save_cursors(path: Path, cursors: dict[str, Cursor]) -> None:
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_bytes(CURSORS.dump_json(cursors))
    os.replace(temporary, path)


def ts(value: datetime) -> str:
    return f"{value.timestamp():.6f}"


def earlier(left: str | None, right: str) -> str | None:
    """None (all history) is earliest."""
    if left is None:
        return None
    return left if Decimal(left) <= Decimal(right) else right


async def workspace(slack: AsyncWebClient, team_id: str) -> Workspace:
    response = await slack.auth_test()
    if response["team_id"] != team_id:
        raise WrongWorkspace(f"the token is for team {response['team_id']}")
    return Workspace(id=team_id, name=response["team"], url=response["url"])


async def history(
    slack: AsyncWebClient, channel: str, oldest: str | None
) -> list[Message]:
    """Top-level messages and thread parents after `oldest`, newest first."""
    found: list[Message] = []
    first = await slack.conversations_history(
        channel=channel, oldest=oldest or "0", limit=PAGE
    )
    async for page in first:
        found += MESSAGES.validate_python(page["messages"])
    return found


async def thread(slack: AsyncWebClient, channel: str, parent: str) -> list[Message]:
    """The parent and all its replies."""
    found: list[Message] = []
    first = await slack.conversations_replies(channel=channel, ts=parent, limit=PAGE)
    async for page in first:
        found += MESSAGES.validate_python(page["messages"])
    return found


async def resolve(slack: AsyncWebClient, people: People, ids: set[str]) -> None:
    """Look up each user once per poll; a failed lookup keeps the bare ID."""
    for id in sorted(ids - people.keys()):
        try:
            response = await slack.users_info(user=id)
            people[id] = User.model_validate(response["user"])
        except (SlackApiError, ValidationError):
            people[id] = None


def failure(error: Exception) -> str:
    if isinstance(error, SlackApiError):
        return str(error.response.get("error", f"HTTP {error.response.status_code}"))
    if isinstance(error, ValidationError):
        return "unexpected response"
    return "network error"


async def channel_snapshot(
    slack: AsyncWebClient,
    settings: Settings,
    team: Workspace,
    people: People,
    channel_id: str,
    cursor: Cursor | None,
    now: datetime,
    result: Result,
) -> tuple[Snapshot | None, Cursor | None]:
    cutoff = settings.cutoff(now)
    window = None if cutoff is None else ts(cutoff)
    lookback = ts(now - timedelta(days=settings.thread_days))
    channel = Channel.model_validate(
        (await slack.conversations_info(channel=channel_id))["channel"]
    )
    start = window if cursor is None else cursor.newest
    listed = await history(slack, channel_id, earlier(start, lookback))
    known = {} if cursor is None else cursor.threads
    found = {item.ts: item for item in listed}
    threads: dict[str, str] = {}
    for item in listed:
        if item.reply_count == 0 or item.latest_reply is None:
            continue
        threads[item.ts] = item.latest_reply
        if known.get(item.ts) != item.latest_reply:
            result.fetched_threads += 1
            for reply in await thread(slack, channel_id, item.ts):
                found.setdefault(reply.ts, reply)
    result.listed_messages += len(found)
    content = [item for item in found.values() if item.subtype in CONTENT]
    kept = [
        item
        for item in content
        if window is None or Decimal(item.ts) >= Decimal(window)
    ]
    result.time_excluded_messages += len(content) - len(kept)
    stamps = [item.ts for item in listed] + ([] if cursor is None else [cursor.newest])
    newest = max(stamps, key=lambda stamp: Decimal(stamp), default=None)
    following = None if newest is None else Cursor(newest=newest, threads=threads)
    if not kept:
        return None, following
    ids = {id for item in kept for id in (item.user, *mentions(item)) if id}
    await resolve(slack, people, ids | ({channel.user} if channel.user else set()))
    # Incomplete: the poll sees only part of the channel, so nothing is deleted.
    snapshot = Snapshot(
        container=chat(team, channel, people),
        objects=[message(team, channel_id, item, people) for item in kept],
        complete=False,
        created_since=cutoff,
    )
    return snapshot, following


async def synchronize(
    settings: Settings,
    slack: AsyncWebClient,
    client: Connector,
    now: datetime | None = None,
) -> Result:
    """Deliver each channel, then advance its cursor; a failure leaves it put."""
    now = now or datetime.now(UTC)
    path = cursor_path(settings)
    cursors = load_cursors(path)
    result = Result(settings.source_id, 0, 0, 0, 0, 0, [])
    try:
        team = await workspace(slack, settings.team_id)
    except (SlackApiError, ClientError, TimeoutError) as error:
        result.issues.append(f"auth.test: {failure(error)}")
        return result
    people: People = {}
    for channel_id in settings.channels:
        try:
            snapshot, cursor = await channel_snapshot(
                slack,
                settings,
                team,
                people,
                channel_id,
                cursors.get(channel_id),
                now,
                result,
            )
        except (SlackApiError, ClientError, TimeoutError, ValidationError) as error:
            result.issues.append(f"{channel_id}: {failure(error)}")
            continue
        if snapshot is not None:
            result.delivered_batches += await client.sync(snapshot)
            result.synced_channels += 1
        if cursor is not None:
            cursors[channel_id] = cursor
            save_cursors(path, cursors)
    return result


async def run(
    settings: Settings,
    slack: AsyncWebClient,
    *,
    watch: bool = False,
    report: Callable[[Result], None],
) -> None:
    settings.state_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    retry_delay = settings.poll_seconds
    while True:
        try:
            async with Connector(
                source_id=settings.source_id,
                modes=[Mode.snapshot],
                destinations=settings.targets(),
                state_path=settings.state_dir / "delivery.sqlite3",
            ) as client:
                await client.send(Changes.declare(*KINDS))
                while True:
                    report(await synchronize(settings, slack, client))
                    if not watch:
                        return
                    retry_delay = settings.poll_seconds
                    await asyncio.sleep(settings.poll_seconds)
        except DeliveryError as error:
            if not watch or not error.retryable:
                raise
            # Reopening resumes saved delivery before the next poll.
            await asyncio.sleep(retry_delay)
            retry_delay = min(max(retry_delay * 2, 1), 60)
