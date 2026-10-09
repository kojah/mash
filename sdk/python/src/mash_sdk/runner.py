"""Inputs to `Connector.run`: snapshots for backfill and captured live events."""

import math
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import dataclass
from datetime import datetime
from typing import Generic, TypeVar

from ._generated.batch import ChangeUpsertObject, Role

T = TypeVar("T")


@dataclass(frozen=True)
class Snapshot:
    """One container and objects below it, as the source currently has them.

    Only a `complete` snapshot may delete by omission, and only entries whose
    nearest container is this one (entries under its segments included),
    created at or after `created_since`. Nested containers, such as reply
    threads, are their own scopes. Entries with unknown creation time are never
    omitted from a bounded window. Pagination must finish first.
    """

    container: ChangeUpsertObject
    objects: Sequence[ChangeUpsertObject]
    complete: bool
    created_since: datetime | None = None

    def __post_init__(self) -> None:
        if self.container.role != Role.container:
            raise ValueError("a snapshot is rooted at a container")
        parents = {item.id: item.parent for item in self.objects}
        for item in self.objects:
            seen = {item.id}
            parent = item.parent
            while parent != self.container.id:
                if parent is None or parent in seen or parent not in parents:
                    raise ValueError("snapshot objects must sit below its container")
                seen.add(parent)
                parent = parents[parent]


@dataclass(frozen=True)
class DocumentSnapshot:
    """An authoritative enumeration of documents inside explicit source scopes.

    A `complete` snapshot deletes documents that an earlier complete snapshot
    delivered from within these scopes and that are now absent. Documents only
    ever sent through events or `send` are never deleted by omission.
    """

    documents: Sequence[ChangeUpsertObject]
    scope_ids: Sequence[str]
    complete: bool
    created_since: datetime | None = None

    def __post_init__(self) -> None:
        if self.complete and not self.scope_ids:
            raise ValueError("a complete document snapshot must name its scopes")
        if any(document.role != Role.document for document in self.documents):
            raise ValueError("a document snapshot holds documents only")


@dataclass(frozen=True)
class Debounce:
    """Refresh a key once it is quiet, or after waiting at most `max_wait_seconds`."""

    quiet_seconds: float
    max_wait_seconds: float

    def __post_init__(self) -> None:
        if not (
            math.isfinite(self.quiet_seconds)
            and math.isfinite(self.max_wait_seconds)
            and 0 <= self.quiet_seconds <= self.max_wait_seconds
        ):
            raise ValueError("debounce needs 0 <= quiet_seconds <= max_wait_seconds")


@dataclass(frozen=True)
class Delivery(Generic[T]):
    """A captured source event. Events with the same `key` coalesce; the latest wins.

    The SDK journals the value before calling `acknowledge`, the source's own
    capture acknowledgement (a webhook reply, a gateway resume point). That is
    distinct from Mash's durable silver receipt. Sources without one omit it.
    """

    id: str
    key: str
    value: T
    acknowledge: Callable[[], Awaitable[None]] | None = None

    async def acknowledge_capture(self) -> None:
        if self.acknowledge is not None:
            await self.acknowledge()
