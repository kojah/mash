"""Desired SDK interface. This is a typing specification, not a runtime SDK."""

from collections.abc import AsyncIterable, Awaitable, Callable, Sequence
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from pathlib import Path
from typing import Generic, Self, TypeAlias, TypeVar

from pydantic import JsonValue

Metadata: TypeAlias = dict[str, JsonValue]
_T = TypeVar("_T")

class AuthorKind(StrEnum):
    human = "human"
    bot = "bot"
    unknown = "unknown"

class Author:
    id: str
    name: str | None
    def __init__(
        self,
        *,
        id: str,
        name: str | None = ...,
        kind: AuthorKind = ...,
    ) -> None: ...

class Attachment:
    def __init__(
        self, *, id: str, name: str, media_type: str, url: str, bytes: int | None = ...
    ) -> None: ...

class Role(StrEnum):
    """What Mash may do with an object; `kind` is the source's own label."""

    container = "container"
    segment = "segment"
    entry = "entry"
    document = "document"

class Object:
    """One source object. Roles decide which fields apply: entries and
    documents carry content; only entries have authors and replies; entries
    need a parent; documents never contain anything."""

    id: str
    role: Role
    kind: str
    parent: str | None
    title: str | None
    content: str | None
    author: Author | None
    metadata: Metadata | None
    def __init__(
        self,
        *,
        id: str,
        role: Role,
        kind: str,
        parent: str | None = ...,
        title: str | None = ...,
        content: str | None = ...,
        author: Author | None = ...,
        reply_to: str | None = ...,
        url: str | None = ...,
        created_at: int | None = ...,
        updated_at: int | None = ...,
        metadata: Metadata | None = ...,
        attachments: Sequence[Attachment] = ...,
    ) -> None: ...

class KindRule:
    """One declared kind: its role, the kinds it may sit under, and whether it
    may be top-level. Entries and segments can never be top-level."""

    def __init__(
        self,
        *,
        kind: str,
        role: Role,
        parents: Sequence[str] = ...,
        root: bool = ...,
    ) -> None: ...

class Receipt:
    source_id: str
    batch_id: str
    source_revision: int
    silver_revision: int
    gold_ready: bool

class Changes:
    @classmethod
    def empty(cls) -> Changes: ...
    @classmethod
    def declare(cls, *rules: KindRule) -> Changes: ...
    @classmethod
    def upsert(cls, *objects: Object) -> Changes: ...
    @classmethod
    def delete(cls, *ids: str) -> Changes: ...

@dataclass(frozen=True)
class Snapshot:
    """One container and objects below it. Only a complete snapshot deletes by
    omission, and only entries whose nearest container is this one."""

    container: Object
    objects: Sequence[Object]
    complete: bool
    created_since: datetime | None = None

@dataclass(frozen=True)
class DocumentSnapshot:
    """Authoritative enumeration of documents inside an explicit source scope."""

    documents: Sequence[Object]
    scope_ids: Sequence[str]
    complete: bool
    created_since: datetime | None = None

@dataclass(frozen=True)
class Debounce:
    quiet_seconds: float
    max_wait_seconds: float

class Delivery(Generic[_T]):
    """SDK journals the event before this optional source-capture callback.

    Webhooks acknowledge upstream capture; Gateway/poll adapters omit it.
    This is distinct from Mash's durable silver receipt and gold readiness.
    Events with the same key coalesce for debounce; the latest value wins.
    """

    id: str
    key: str
    value: _T
    def __init__(
        self,
        *,
        id: str,
        key: str,
        value: _T,
        acknowledge: Callable[[], Awaitable[None]] | None = ...,
    ) -> None: ...
    async def acknowledge_capture(self) -> None: ...

class Destination:
    @classmethod
    def local(
        cls,
        *,
        database: Path,
        executable: str = ...,
        config: Path | None = ...,
        timeout: float = ...,
    ) -> Destination: ...
    @classmethod
    def shared(
        cls,
        *,
        api_url: str,
        group_id: str,
        node_id: str,
        token_env: str,
        timeout: float = ...,
    ) -> Destination: ...

class Connector:
    def __init__(
        self,
        *,
        source_id: str,
        destinations: Sequence[Destination],
        state_path: Path,
    ) -> None: ...
    async def __aenter__(self) -> Self: ...
    async def __aexit__(self, *args: object) -> None: ...
    async def send(self, changes: Changes) -> tuple[Receipt, ...]: ...
    async def sync(self, *snapshots: Snapshot | DocumentSnapshot) -> int: ...
    async def run(
        self,
        *,
        backfill: AsyncIterable[Snapshot | DocumentSnapshot],
        events: AsyncIterable[Delivery[_T]],
        event_type: type[_T],
        refresh: Callable[[_T], Awaitable[Changes]],
        debounce: Debounce,
    ) -> None: ...

@dataclass(frozen=True)
class SilverSnapshot:
    source_id: str
    revision: str
    container: Object
    objects: Sequence[Object]

@dataclass(frozen=True)
class DocumentSilverSnapshot:
    source_id: str
    revision: str
    document: Object

@dataclass(frozen=True)
class MessageRef:
    source_id: str
    chat_id: str
    message_id: str

@dataclass(frozen=True)
class DocumentRef:
    source_id: str
    document_id: str

@dataclass(frozen=True)
class SearchDocument:
    id: str
    text: str
    references: Sequence[MessageRef | DocumentRef]
    metadata: Metadata

class Projector:
    """Reads silver; publishes gold with SDK-managed dependency tokens."""
    def __init__(
        self,
        *,
        artifact: str,
        recipe_version: str,
        destination: Destination,
        state_path: Path,
    ) -> None: ...
    async def run(
        self,
        *,
        transform: Callable[
            [SilverSnapshot | DocumentSilverSnapshot],
            Awaitable[Sequence[SearchDocument]],
        ],
    ) -> None: ...
