"""Common note mapping, independent of vault files or Apple Notes database layout."""

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
    Document,
    DocumentSnapshot,
    Metadata,
)

from .common import File, milliseconds
from .scope import Scope


@dataclass(frozen=True)
class Note:
    id: str
    folder_id: str
    title: str
    content: str
    created_at: datetime | None
    updated_at: datetime | None
    metadata: Metadata
    files: Sequence[File]


@dataclass(frozen=True)
class Event:
    id: str
    folder_id: str
    deleted: bool = False


class Notes(Protocol):
    """Only a successful complete enumeration may infer deletion from absence."""

    def notes(
        self, folders: Sequence[str], *, since: datetime | None
    ) -> AsyncIterator[Note]: ...
    async def note(self, id: str) -> Note: ...
    def events(self, folders: Sequence[str]) -> AsyncIterator[Delivery[Event]]: ...


def document(note: Note) -> Document:
    return Document(
        id=note.id,
        title=note.title,
        content=note.content,
        created_at=milliseconds(note.created_at),
        updated_at=milliseconds(note.updated_at),
        metadata={**note.metadata, "folder_id": note.folder_id},
        attachments=[file.to_attachment() for file in note.files],
    )


async def backfill(source: Notes, scope: Scope) -> AsyncIterator[DocumentSnapshot]:
    since = scope.cutoff()
    notes = [
        document(note)
        async for note in source.notes(scope.ids, since=since)
        if note.folder_id in scope.ids
        and (since is None or note.created_at is not None and note.created_at >= since)
    ]
    yield DocumentSnapshot(
        documents=notes, scope_ids=scope.ids, complete=True, created_since=since
    )


async def refresh(source: Notes, event: Event, scope: Scope) -> Changes:
    if event.folder_id not in scope.ids:
        return Changes.empty()
    if event.deleted:
        # Disappearance during file saves, moves, or failed DB reads is not proof.
        return Changes.delete_documents(event.id)
    note = await source.note(event.id)
    if not scope.includes(note.folder_id, note.created_at):
        return Changes.empty()
    return Changes.upsert(document(note))


async def run(
    source: Notes,
    *,
    source_id: str,
    scope: Scope,
    destination: Destination,
    state_path: Path,
) -> None:
    async with Connector(
        source_id=source_id, destination=destination, state_path=state_path
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
