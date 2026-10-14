"""Polling and delivery; the Mash SDK owns retries, receipts and batch identities."""

import asyncio
from collections.abc import Callable
from dataclasses import asdict, dataclass
from typing import TypeVar

from mash_sdk import Changes, Connector, DeliveryError, DocumentSnapshot, Mode

from .config import Settings
from .registry import Registry
from .vault import scan

T = TypeVar("T")


async def finish(work: Callable[[], T]) -> T:
    # Cancellation must not close the registry while its blocking scan/commit
    # still uses that connection. Finish accepted local work before cleanup.
    task = asyncio.create_task(asyncio.to_thread(work))
    try:
        return await asyncio.shield(task)
    except asyncio.CancelledError:
        await task
        raise


@dataclass(frozen=True)
class Result:
    source_id: str
    scanned_notes: int
    selected_notes: int
    time_excluded_notes: int
    deleted_notes: int
    delivered_batches: int
    issues: list[str]
    local_views_require_refresh: bool

    def json(self) -> dict[str, object]:
        return asdict(self)


async def synchronize(
    settings: Settings, registry: Registry, client: Connector
) -> Result:
    current = await finish(lambda: scan(settings, registry))
    # Omission is deliberately disabled: a transient missing note, narrower
    # folder scope or unsuccessful read is not a confirmed deletion.
    count = await client.sync(
        DocumentSnapshot(
            documents=current.documents,
            scope_ids=list(settings.folders),
            complete=False,
        )
    )
    for start in range(0, len(current.deleted), 512):
        ids = current.deleted[start : start + 512]
        await client.send(Changes.delete(*ids))
        count += 1
        await finish(lambda ids=ids: registry.deleted(ids))
    return Result(
        settings.source_id,
        current.scanned,
        len(current.documents),
        current.excluded,
        len(current.deleted),
        count,
        current.issues,
        count > 0 and any(item.type == "local" for item in settings.destinations),
    )


async def run(
    settings: Settings, *, watch: bool = False, report: Callable[[Result], None]
) -> None:
    with Registry(settings.state_dir, settings.vault_id) as registry:
        retry_delay = settings.poll_seconds
        while True:
            try:
                async with Connector(
                    source_id=settings.source_id,
                    modes=[Mode.snapshot],
                    destinations=settings.targets(),
                    state_path=settings.state_dir / "delivery.sqlite3",
                ) as client:
                    while True:
                        report(await synchronize(settings, registry, client))
                        if not watch:
                            return
                        retry_delay = settings.poll_seconds
                        await asyncio.sleep(settings.poll_seconds)
            except DeliveryError as error:
                if not watch or not error.retryable:
                    raise
                # Reopening resumes saved work before another scan. No vault
                # text, bearer tokens or raw HTTP response bodies enter logs.
                await asyncio.sleep(retry_delay)
                retry_delay = min(max(retry_delay * 2, 1), 60)
