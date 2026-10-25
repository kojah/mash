"""Polling and delivery; the Mash SDK owns retries, receipts and batch identities.

Each poll lists the selected scopes (cheap metadata calls) and exports only Docs
whose Drive `version` differs from the last delivered one. Listing every poll
keeps scope changes and new Docs simple; the Changes API would need its own
ancestry checks to decide whether a changed file is inside a selected folder.
"""

import asyncio
import hashlib
import os
from collections.abc import Callable, Sequence
from dataclasses import asdict, dataclass, field
from datetime import UTC, datetime, timedelta
from pathlib import Path

import orjson
from mash_sdk import Changes, Connector, DeliveryError, DocumentSnapshot, Mode
from pydantic import BaseModel, TypeAdapter

from .api import HttpError
from .config import Settings
from .drive import DOCUMENT, FOLDER, Comment, Drive, File
from .objects import KINDS, comments_id, discussion, document, document_id

# Docs are exported and delivered in groups so a long first backfill records
# progress as it goes instead of holding every export in memory.
CHUNK = 25


class Delivered(BaseModel):
    """What was last delivered for a Doc, so unchanged Docs are not re-exported."""

    version: str
    comments: bool


SEEN = TypeAdapter(dict[str, Delivered])


@dataclass
class Selection:
    documents: list[File] = field(default_factory=list)
    trashed: list[File] = field(default_factory=list)
    excluded: int = 0
    issues: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class Result:
    source_id: str
    selected_documents: int
    exported_documents: int
    time_excluded_documents: int
    deleted_documents: int
    delivered_batches: int
    issues: list[str]

    def json(self) -> dict[str, object]:
        return asdict(self)


def failure(error: HttpError | OSError) -> str:
    return f"HTTP {error.status}" if isinstance(error, HttpError) else "network error"


def select(drive: Drive, settings: Settings, now: datetime) -> Selection:
    """Selected Docs; failures become issues and never imply deletion."""
    found: dict[str, File] = {}
    issues: list[str] = []
    pending = list(settings.folders)
    visited: set[str] = set()
    while pending:
        folder = pending.pop()
        if folder in visited:
            continue
        visited.add(folder)
        try:
            for item in drive.children(folder):
                if item.mime_type == FOLDER:
                    pending.append(item.id)
                else:
                    found[item.id] = item
        except (HttpError, OSError) as error:
            issues.append(f"folder {folder}: {failure(error)}")
    for drive_id in settings.shared_drives:
        try:
            found.update((item.id, item) for item in drive.shared_drive(drive_id))
        except (HttpError, OSError) as error:
            issues.append(f"shared drive {drive_id}: {failure(error)}")
    for file_id in settings.documents:
        try:
            found[file_id] = drive.file(file_id)
        except (HttpError, OSError) as error:
            issues.append(f"document {file_id}: {failure(error)}")
    cutoff = (
        None
        if settings.history_days is None
        else now - timedelta(days=settings.history_days)
    )
    selection = Selection(issues=issues)
    for item in found.values():
        if item.mime_type != DOCUMENT:
            continue
        if item.trashed:
            selection.trashed.append(item)
        elif cutoff is not None and item.created_time < cutoff:
            selection.excluded += 1
        else:
            selection.documents.append(item)
    return selection


def load_seen(path: Path) -> dict[str, Delivered]:
    return SEEN.validate_json(path.read_bytes()) if path.exists() else {}


def save_seen(path: Path, seen: dict[str, Delivered]) -> None:
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_bytes(SEEN.dump_json(seen))
    os.replace(temporary, path)


def export(
    drive: Drive, files: Sequence[File], issues: list[str]
) -> list[tuple[File, str, list[Comment]]]:
    exported: list[tuple[File, str, list[Comment]]] = []
    for item in files:
        try:
            exported.append((item, drive.export(item.id), drive.comments(item.id)))
        except (HttpError, OSError) as error:
            # Unshared or deleted between listing and export; retried next poll.
            issues.append(f"document {item.id}: {failure(error)}")
    return exported


async def synchronize(
    settings: Settings, drive: Drive, client: Connector, now: datetime | None = None
) -> Result:
    current = now or datetime.now(UTC)
    selection = await asyncio.to_thread(select, drive, settings, current)
    # Keyed by the destination set, so an added destination gets a full backfill
    # (the SDK still skips what each destination already acknowledged).
    destinations = orjson.dumps(
        [item.model_dump(mode="json") for item in settings.destinations]
    )
    seen_path = (
        settings.state_dir
        / f"documents-{hashlib.sha256(destinations).hexdigest()[:16]}.json"
    )
    seen = load_seen(seen_path)
    changed = [
        item
        for item in selection.documents
        if item.id not in seen or seen[item.id].version != item.version
    ]
    batches = exported = 0
    for start in range(0, len(changed), CHUNK):
        group = await asyncio.to_thread(
            export, drive, changed[start : start + CHUNK], selection.issues
        )
        if not group:
            continue
        snapshots = [
            discussion(item, comments)
            for item, _, comments in group
            # An empty container is only worth sending to clear old comments.
            if comments or (item.id in seen and seen[item.id].comments)
        ]
        # Not complete: an unlisted Doc may be unshared or out of scope, which
        # is never a deletion. Trash is the only deletion signal used below.
        batches += await client.sync(
            DocumentSnapshot(
                documents=[document(item, text) for item, text, _ in group],
                scope_ids=[],
                complete=False,
            ),
            *snapshots,
        )
        exported += len(group)
        for item, _, comments in group:
            had = item.id in seen and seen[item.id].comments
            seen[item.id] = Delivered(
                version=item.version, comments=bool(comments) or had
            )
        save_seen(seen_path, seen)
    trashed = [item.id for item in selection.trashed if item.id in seen]
    for file_id in trashed:
        ids = [document_id(file_id)]
        if seen[file_id].comments:
            ids.append(comments_id(file_id))
        await client.send(Changes.delete(*ids))
        batches += 1
        del seen[file_id]
        save_seen(seen_path, seen)
    return Result(
        settings.source_id,
        len(selection.documents),
        exported,
        selection.excluded,
        len(trashed),
        batches,
        selection.issues,
    )


async def run(
    settings: Settings,
    drive: Drive,
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
                    report(await synchronize(settings, drive, client))
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
