"""Polling and delivery; the Mash SDK owns retries, receipts and batch identities.

Each poll asks every repository for issues and PRs updated since its cursor.
New comments, reviews and review comments all bump their issue's `updated_at`,
so an old issue with recent discussion is listed again. Each listed item's
discussion is refetched in full for the history window and synced as a
complete snapshot; the SDK skips entries Mash already has. Webhooks would need
a public endpoint, so there is no event stream.
"""

import asyncio
import hashlib
import os
from collections.abc import Callable
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path

import orjson
from githubkit import GitHub, TokenAuthStrategy
from githubkit.exception import RequestError, RequestFailed
from githubkit.utils import UNSET
from githubkit_schemas.latest.models import Issue
from mash_sdk import Changes, Connector, DeliveryError, Mode, Object, Snapshot
from pydantic import BaseModel, TypeAdapter, ValidationError

from .config import ItemKind, Settings
from .objects import (
    KINDS,
    chat,
    comment,
    description,
    milliseconds,
    review,
    review_comment,
)

PAGE = 100


class Cursor(BaseModel):
    """The newest `updated_at` delivered, and which issues had exactly that time.

    GitHub's `since` is inclusive, so the IDs keep the boundary from being
    refetched every poll without missing a same-second update.
    """

    since: datetime
    ids: list[int]


CURSORS = TypeAdapter(dict[str, Cursor])


@dataclass
class Result:
    source_id: str
    listed_items: int
    synced_items: int
    time_excluded_items: int
    delivered_batches: int
    issues: list[str]

    def json(self) -> dict[str, object]:
        return asdict(self)


def cursor_path(settings: Settings) -> Path:
    # Changing destinations, kinds or the window restarts from the window start;
    # the SDK still skips what each destination already acknowledged.
    key = orjson.dumps(
        [
            [item.model_dump(mode="json") for item in settings.destinations],
            settings.item_kinds,
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


async def updated(
    github: GitHub[TokenAuthStrategy],
    repository: str,
    since: datetime | None,
    page: int,
) -> list[Issue]:
    owner, name = repository.split("/")
    response = await github.rest.issues.async_list_for_repo(
        owner,
        name,
        state="all",
        sort="updated",
        direction="asc",
        since=UNSET if since is None else since,
        per_page=PAGE,
        page=page,
    )
    return response.parsed_data


async def discussion(
    github: GitHub[TokenAuthStrategy],
    repository: str,
    issue: Issue,
    cutoff: datetime | None,
) -> Snapshot | None:
    """Everything created in the window; None when nothing is."""
    owner, name = repository.split("/")
    # `since` filters by update time, a superset of creation in the window.
    since = UNSET if cutoff is None else cutoff
    rest = github.rest
    entries: list[Object | None] = [description(issue)]
    async for item in rest.paginate(
        rest.issues.async_list_comments,
        owner=owner,
        repo=name,
        issue_number=issue.number,
        since=since,
        per_page=PAGE,
    ):
        entries.append(comment(issue, item))
    if issue.pull_request:
        async for found in rest.paginate(
            rest.pulls.async_list_reviews,
            owner=owner,
            repo=name,
            pull_number=issue.number,
            per_page=PAGE,
        ):
            entries.append(review(issue, found))
        async for inline in rest.paginate(
            rest.pulls.async_list_review_comments,
            owner=owner,
            repo=name,
            pull_number=issue.number,
            since=since,
            per_page=PAGE,
        ):
            entries.append(review_comment(issue, inline))
    start = milliseconds(cutoff)
    kept = [
        entry
        for entry in entries
        if entry is not None and (start is None or (entry.created_at or 0) >= start)
    ]
    if not kept:
        return None
    # Complete within the window: an entry deleted on GitHub is deleted here.
    return Snapshot(
        container=chat(repository, issue),
        objects=kept,
        complete=True,
        created_since=cutoff,
    )


def failure(error: Exception) -> str:
    if isinstance(error, RequestFailed):
        return f"HTTP {error.response.status_code}"
    if isinstance(error, ValidationError):
        return "unexpected response"
    return "network error"


async def synchronize(
    settings: Settings,
    github: GitHub[TokenAuthStrategy],
    client: Connector,
    now: datetime | None = None,
) -> Result:
    """Advance each repository's cursor item by item, after each delivery.

    Pages are requested from the cursor each time rather than by offset, so an
    issue updated mid-listing moves behind the cursor instead of shifting an
    unseen one onto an already-read page.
    """
    cutoff = settings.cutoff(now or datetime.now(UTC))
    path = cursor_path(settings)
    cursors = load_cursors(path)
    result = Result(settings.source_id, 0, 0, 0, 0, [])
    for repository in settings.repositories:
        cursor = cursors.get(repository)
        page = 1
        try:
            while True:
                since = cutoff if cursor is None else cursor.since
                listed = await updated(github, repository, since, page)
                fresh = [
                    issue
                    for issue in listed
                    if cursor is None
                    or issue.updated_at > cursor.since
                    or issue.id not in cursor.ids
                ]
                for issue in fresh:
                    result.listed_items += 1
                    pull = bool(issue.pull_request)
                    kind = ItemKind.pull_request if pull else ItemKind.issue
                    if kind in settings.item_kinds:
                        snapshot = await discussion(github, repository, issue, cutoff)
                        if snapshot is None:
                            result.time_excluded_items += 1
                        else:
                            result.delivered_batches += await client.sync(snapshot)
                            result.synced_items += 1
                    if cursor is None or issue.updated_at > cursor.since:
                        cursor = Cursor(since=issue.updated_at, ids=[issue.id])
                    else:
                        cursor.ids.append(issue.id)
                    cursors[repository] = cursor
                    save_cursors(path, cursors)
                if len(listed) < PAGE:
                    break
                # A full page of already-delivered same-second updates: skip it.
                page = 1 if fresh else page + 1
        except (RequestFailed, RequestError, ValidationError) as error:
            # The cursor stays at the last delivered item; the next poll resumes.
            result.issues.append(f"{repository}: {failure(error)}")
    return result


async def run(
    settings: Settings,
    github: GitHub[TokenAuthStrategy],
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
                    report(await synchronize(settings, github, client))
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
