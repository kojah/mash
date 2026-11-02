"""Polling and delivery; the Mash SDK owns retries, receipts and batch identities.

Each poll reads every allowlisted channel's messages after a per-channel cursor:
the newest message ID delivered. Snowflakes are ordered by creation time, so the
history window is a snowflake floor too. Messages are never revisited, so later
edits and deletions are not seen, and snapshots are incomplete: omission never
deletes. Live gateway events would need a long-running connection.
"""

import asyncio
import hashlib
import os
from collections.abc import Callable
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path

import hikari
import orjson
from hikari.api import RESTClient
from mash_sdk import Changes, Connector, DeliveryError, Mode, Snapshot
from pydantic import TypeAdapter

from .config import Settings
from .objects import KINDS, chat, message

PAGE = 100  # Discord's maximum page size.
# hikari retries a 429 without limit, each wait capped at MAX_RATE_LIMIT; this
# bounds one request including its retries, so a channel cannot stall a poll.
REQUEST_SECONDS = 180.0
CONTENT = (
    hikari.ApplicationFlags.MESSAGE_CONTENT_INTENT
    | hikari.ApplicationFlags.MESSAGE_CONTENT_INTENT_LIMITED
)
CURSORS = TypeAdapter(dict[str, int])


@dataclass
class Result:
    source_id: str
    fetched_messages: int
    synced_messages: int
    delivered_batches: int
    issues: list[str]

    def json(self) -> dict[str, object]:
        return asdict(self)


def cursor_path(settings: Settings) -> Path:
    # Changing destinations or the window restarts from the window start; the
    # SDK still skips what each destination already acknowledged.
    key = orjson.dumps(
        [
            # The source key is part of the cursor identity: two instances of
            # this connector sharing a state_dir must not share cursors.
            settings.source_id,
            [item.model_dump(mode="json") for item in settings.destinations],
            settings.history_days,
        ]
    )
    return settings.state_dir / f"cursors-{hashlib.sha256(key).hexdigest()[:16]}.json"


def load_cursors(path: Path) -> dict[str, int]:
    return CURSORS.validate_json(path.read_bytes()) if path.exists() else {}


def save_cursors(path: Path, cursors: dict[str, int]) -> None:
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_bytes(CURSORS.dump_json(cursors))
    os.replace(temporary, path)


def failure(error: hikari.errors.HTTPError) -> str:
    if isinstance(error, hikari.errors.RateLimitTooLongError):
        return "rate limited"
    if isinstance(error, hikari.errors.HTTPResponseError):
        return f"HTTP {int(error.status)}"
    return "network error"


async def synchronize(
    settings: Settings,
    rest: RESTClient,
    client: Connector,
    now: datetime | None = None,
) -> Result:
    """Advance each channel's cursor page by page, after each delivery."""
    cutoff = settings.cutoff(now or datetime.now(UTC))
    floor = 0 if cutoff is None else int(hikari.Snowflake.from_datetime(cutoff))
    path = cursor_path(settings)
    cursors = load_cursors(path)
    result = Result(settings.source_id, 0, 0, 0, [])
    try:
        application = await rest.fetch_application()
    except hikari.errors.HTTPError as error:
        result.issues.append(f"application: {failure(error)}")
        return result
    if not application.flags & CONTENT:
        # Without it Discord blanks message text, and cursors would skip past it.
        result.issues.append("application: the Message Content intent is disabled")
        return result
    for id in settings.channels:
        try:
            async with asyncio.timeout(REQUEST_SECONDS):
                channel = await rest.fetch_channel(int(id))
            if not isinstance(channel, hikari.TextableChannel):
                result.issues.append(f"{id}: not a text channel or thread")
                continue
            container = chat(channel)
            start = max(cursors.get(id, 0), floor)
            pages = rest.fetch_messages(channel.id, after=start).chunk(PAGE)
            while True:
                # Only the request is bounded, never delivery to Mash.
                async with asyncio.timeout(REQUEST_SECONDS):
                    page = await anext(pages, None)
                if page is None:
                    break
                result.fetched_messages += len(page)
                entries = [entry for item in page if (entry := message(channel, item))]
                if entries:
                    result.delivered_batches += await client.sync(
                        Snapshot(container=container, objects=entries, complete=False)
                    )
                    result.synced_messages += len(entries)
                cursors[id] = int(page[-1].id)
                save_cursors(path, cursors)
        except TimeoutError:
            result.issues.append(f"{id}: rate limited or timed out")
        except hikari.errors.HTTPError as error:
            # The cursor stays at the last delivered page; the next poll resumes.
            result.issues.append(f"{id}: {failure(error)}")
    return result


async def run(
    settings: Settings,
    rest: RESTClient,
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
                    report(await synchronize(settings, rest, client))
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
