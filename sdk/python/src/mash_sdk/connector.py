"""High-level delivery; revisions, identities and saved batches stay private."""

import asyncio
import hashlib
import sqlite3
import time
from collections.abc import AsyncIterable, Awaitable, Callable, Iterator, Sequence
from contextlib import contextmanager
from pathlib import Path
from types import TracebackType
from typing import Self, TypeVar

from pydantic import TypeAdapter, ValidationError
from pydantic_core import PydanticSerializationError

from ._delivery import DeliveryError
from ._generated.batch import Batch
from ._generated.receipt import Receipt
from ._local import deliver_local
from ._plan import Plan, batches, snapshot_plan, unchanged_removed
from ._receipts import InvalidReceipt
from ._shared import deliver_shared
from ._state import (
    JournaledEvent,
    PendingBatch,
    PendingExists,
    StateError,
    StateStore,
)
from .changes import Changes
from .codec import InvalidBatch, encode_batch
from .destination import Destination, Local, Shared
from .runner import Debounce, Delivery, DocumentSnapshot, Snapshot

T = TypeVar("T")
R = TypeVar("R")
# Retryable delivery failures back off up to this many seconds between attempts.
MAX_RETRY_SECONDS = 60.0


class Connector:
    """Send typed changes to one or more destinations and recover unfinished delivery.

    Each destination (local or shared silver) has its own saved batch, ordering
    and ledger, so a failing destination never blocks or reorders the others.
    Use an async context manager. Opening automatically resumes saved batches.
    Cancellation of a caller does not cancel already accepted work; exit waits
    for outstanding delivery. `run` adds backfill, event capture and debounce.
    Records become searchable once each destination's gold projects them.
    """

    def __init__(
        self,
        *,
        source_id: str,
        destinations: Sequence[Destination],
        state_path: Path,
    ) -> None:
        try:
            TypeAdapter(
                Batch.model_fields["source_id"].rebuild_annotation()
            ).validate_python(source_id)
        except ValidationError:
            raise ValueError("invalid connector source identity") from None
        self.source_id = source_id
        self.destinations = tuple(destinations)
        identities = [destination._identity() for destination in self.destinations]
        if not identities:
            raise ValueError("a connector needs at least one destination")
        if len(set(identities)) != len(identities):
            raise ValueError("connector destinations must be distinct")
        # Captured events are journaled once per connector and complete only
        # when every destination has their refresh.
        self._journal_id = (
            identities[0]
            if len(identities) == 1
            else "fanout:"
            + hashlib.sha256("\0".join(sorted(identities)).encode()).hexdigest()
        )
        self.state_path = state_path.resolve()
        self._lock = asyncio.Lock()
        self._tasks: set[asyncio.Task[object]] = set()
        self._entered = False

    async def __aenter__(self) -> Self:
        if self._entered:
            raise RuntimeError("connector is already open")
        self._entered = True
        try:
            await self.resume()
        except BaseException:
            self._entered = False
            raise
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self._entered = False
        if self._tasks:
            await asyncio.shield(
                asyncio.gather(*tuple(self._tasks), return_exceptions=True)
            )

    async def send(self, changes: Changes) -> tuple[Receipt, ...]:
        """Persist and deliver one batch to every destination, in their order.

        Every destination is attempted even if another fails; a failure leaves
        that destination's batch saved for recovery and raises after the rest.
        Retrying identical changes while a batch is pending reuses its receipt.
        Different changes wait for that destination's pending work first.
        """
        if not changes.mutations:
            raise InvalidBatch("cannot send an empty batch")
        captured = Changes(
            tuple(item.model_copy(deep=True) for item in changes.mutations)
        )
        results = await self._call(
            lambda: self._each(lambda destination: self._send(destination, captured))
        )
        receipts = tuple(result for result in results if result is not None)
        if len(receipts) != len(results):
            raise RuntimeError("delivery returned no receipt")
        return receipts

    async def run(
        self,
        *,
        backfill: AsyncIterable[Snapshot | DocumentSnapshot],
        events: AsyncIterable[Delivery[T]],
        event_type: type[T],
        refresh: Callable[[T], Awaitable[Changes]],
        debounce: Debounce,
    ) -> None:
        """Backfill, then keep refreshing captured events until `events` ends.

        Capture starts before backfill so nothing changing during backfill is
        missed. Each event is journaled durably, typed by `event_type`, before
        its source acknowledgement. Refreshes wait until backfill has finished,
        so a refresh's current state is never overwritten by an older snapshot,
        and a journal left by an earlier run is refreshed on restart.
        Unchanged records are not resent. Retryable delivery failures back off
        and retry; other failures, including refresh errors, end the run with
        the event still journaled.
        """
        if not self._entered:
            raise RuntimeError("use the connector inside an async context manager")
        adapter = TypeAdapter(event_type)
        destination_id = self._journal_id
        wake = asyncio.Event()

        async def capture() -> None:
            try:
                async for delivery in events:
                    try:
                        payload = adapter.dump_json(delivery.value)
                    except PydanticSerializationError:
                        raise ValueError(
                            "event value cannot be serialized as event_type"
                        ) from None
                    await asyncio.to_thread(
                        self._journal, destination_id, delivery.key, payload
                    )
                    await delivery.acknowledge_capture()
                    wake.set()
            finally:
                wake.set()

        capturing = asyncio.create_task(capture())
        try:
            async for snapshot in backfill:
                await self._retrying(
                    lambda snapshot=snapshot: self._publish(
                        lambda store, id: snapshot_plan(
                            store, id, self.source_id, snapshot
                        )
                    ),
                    capturing,
                )
            while True:
                # Clear before reading so a capture that lands meanwhile wakes us.
                wake.clear()
                due, upcoming = await asyncio.to_thread(
                    self._due, destination_id, debounce
                )
                for event in due:
                    await self._refresh(event, adapter, refresh, capturing)
                if due:
                    continue
                if capturing.done():
                    capturing.result()  # Re-raise a capture failure.
                    if upcoming is None:
                        return
                timeout = None if upcoming is None else max(0.0, upcoming - time.time())
                try:
                    await asyncio.wait_for(wake.wait(), timeout)
                except TimeoutError:
                    pass
        finally:
            capturing.cancel()
            await asyncio.gather(capturing, return_exceptions=True)

    async def _refresh(
        self,
        event: JournaledEvent,
        adapter: TypeAdapter[T],
        refresh: Callable[[T], Awaitable[Changes]],
        capturing: asyncio.Task[None],
    ) -> None:
        try:
            value = adapter.validate_json(event.payload)
        except ValidationError:
            raise DeliveryError(
                "a journaled event no longer matches event_type", retryable=False
            ) from None
        changes = await refresh(value)
        if changes.mutations:
            await self._retrying(
                lambda: self._publish(
                    lambda store, id: Plan(
                        unchanged_removed(store, id, self.source_id, changes)
                    )
                ),
                capturing,
            )
        await asyncio.to_thread(self._complete, self._journal_id, event)

    async def _retrying(
        self, operation: Callable[[], int], capturing: asyncio.Task[None]
    ) -> None:
        delay = 1.0
        while True:
            try:
                await self._call(operation)
                return
            except DeliveryError as error:
                if not error.retryable:
                    raise
            if capturing.done() and capturing.exception() is not None:
                capturing.result()
            await asyncio.sleep(delay)
            delay = min(delay * 2, MAX_RETRY_SECONDS)

    def _publish(self, plan: Callable[[StateStore, str], Plan]) -> int:
        """Plan and send separately to each destination against its own ledger."""
        return sum(self._each(lambda destination: self._publish_to(destination, plan)))

    def _publish_to(
        self, destination: Destination, plan: Callable[[StateStore, str], Plan]
    ) -> int:
        """Finish saved work, then plan against the ledger and send each part."""
        destination_id = destination._identity()
        with translated(), StateStore(self.state_path) as store:
            pending = store.pending(
                source_id=self.source_id, destination_id=destination_id
            )
            if pending is not None:
                self._deliver(store, pending, destination)
            sent = 0
            for part in batches(plan(store, destination_id), store.contract):
                prepared = store.prepare(
                    part.changes,
                    source_id=self.source_id,
                    destination_id=destination_id,
                    document_scopes=part.document_scopes,
                )
                self._deliver(store, prepared, destination)
                sent += 1
            return sent

    def _journal(self, destination_id: str, key: str, payload: bytes) -> None:
        with translated(), StateStore(self.state_path) as store:
            store.capture(
                destination_id=destination_id,
                source_id=self.source_id,
                key=key,
                payload=payload,
                now=time.time(),
            )

    def _due(
        self, destination_id: str, debounce: Debounce
    ) -> tuple[list[JournaledEvent], float | None]:
        with translated(), StateStore(self.state_path) as store:
            return store.due_events(
                destination_id=destination_id,
                source_id=self.source_id,
                now=time.time(),
                quiet_seconds=debounce.quiet_seconds,
                max_wait_seconds=debounce.max_wait_seconds,
            )

    def _complete(self, destination_id: str, event: JournaledEvent) -> None:
        with translated(), StateStore(self.state_path) as store:
            store.complete_event(
                destination_id=destination_id,
                source_id=self.source_id,
                key=event.key,
                sequence=event.sequence,
            )

    async def resume(self) -> tuple[Receipt | None, ...]:
        """Retry saved delivery, if any; normally performed automatically on open.

        Returns one entry per destination: its receipt, or None if nothing was saved.
        """
        return await self._call(
            lambda: self._each(lambda destination: self._send(destination, None))
        )

    async def _call(self, operation: Callable[[], R]) -> R:
        if not self._entered:
            raise RuntimeError("use the connector inside an async context manager")

        async def serialized() -> R:
            # The task owns the lock, so cancelling its caller cannot allow a
            # later send to race the still-running blocking delivery.
            async with self._lock:
                return await asyncio.to_thread(operation)

        task = asyncio.create_task(serialized())
        self._tasks.add(task)
        task.add_done_callback(self._finished)
        return await asyncio.shield(task)

    def _finished(self, task: asyncio.Task[object]) -> None:
        self._tasks.discard(task)
        if not task.cancelled():
            task.exception()  # Consume errors even if its caller was cancelled.

    def _each(self, operation: Callable[[Destination], R]) -> tuple[R, ...]:
        """Run per destination, attempting all before reporting failures.

        Retryable only if every failure is: a permanent failure needs attention
        even when another destination would recover by itself.
        """
        results: list[R] = []
        errors: list[DeliveryError] = []
        for destination in self.destinations:
            try:
                results.append(operation(destination))
            except DeliveryError as error:
                errors.append(error)
        if not errors:
            return tuple(results)
        if len(self.destinations) == 1:
            raise errors[0]
        raise DeliveryError(
            f"{len(errors)} of {len(self.destinations)} destinations failed: "
            + "; ".join(str(error) for error in errors),
            retryable=all(error.retryable for error in errors),
        )

    def _send(
        self, destination: Destination, changes: Changes | None
    ) -> Receipt | None:
        destination_id = destination._identity()
        with translated(), StateStore(self.state_path) as store:
            pending = store.pending(
                source_id=self.source_id, destination_id=destination_id
            )
            if pending is not None:
                same = (
                    changes is not None
                    and encode_batch(
                        changes.to_batch(
                            source_id=self.source_id,
                            batch_id=pending.batch_id,
                            expected_revision=pending.expected_revision,
                        ),
                        contract=store.contract,
                    )
                    == pending.payload
                )
                receipt = self._deliver(store, pending, destination)
                if same or changes is None:
                    return receipt
            if changes is None:
                return None
            return self._deliver(
                store,
                store.prepare(
                    changes, source_id=self.source_id, destination_id=destination_id
                ),
                destination,
            )

    def _deliver(
        self, store: StateStore, pending: PendingBatch, destination: Destination
    ) -> Receipt:
        timeout = destination.timeout
        match destination.target:
            case Local() as local:
                payload = deliver_local(
                    local, timeout, pending.payload, self.state_path
                )
            case Shared() as shared:
                payload = deliver_shared(shared, timeout, pending.payload)
        return store.acknowledge(
            payload,
            source_id=self.source_id,
            destination_id=destination._identity(),
        )


@contextmanager
def translated() -> Iterator[None]:
    """Map state, schema and receipt failures to content-free SDK errors."""
    try:
        yield
    except ValidationError:
        raise InvalidBatch("connector changes do not match the schema") from None
    except PendingExists:
        raise DeliveryError(
            "another connector delivery is pending", retryable=True
        ) from None
    except StateError:
        raise DeliveryError(
            "connector delivery state needs attention", retryable=False
        ) from None
    except (sqlite3.Error, InvalidReceipt, OSError):
        raise DeliveryError(
            "connector delivery state or receipt is invalid", retryable=True
        ) from None
