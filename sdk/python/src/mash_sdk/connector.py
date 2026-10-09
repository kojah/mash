"""High-level local delivery; revisions, identities and saved batches stay private."""

import asyncio
import sqlite3
from collections.abc import Callable
from pathlib import Path
from types import TracebackType
from typing import Self

from pydantic import TypeAdapter, ValidationError

from ._generated.batch import Batch
from ._generated.receipt import Receipt
from ._local import DeliveryError, deliver
from ._receipts import InvalidReceipt
from ._state import PendingBatch, PendingExists, StateError, StateStore
from .changes import Changes
from .codec import InvalidBatch, encode_batch
from .destination import Destination


class Connector:
    """Send typed changes to local silver and recover unfinished delivery.

    Use an async context manager. Opening automatically resumes a saved batch.
    Cancellation of a caller does not cancel already accepted work; exit waits
    for outstanding delivery. This client does not yet implement event capture,
    debounce, snapshot diffing, shared delivery, or gold projection.
    """

    def __init__(
        self, *, source_id: str, destination: Destination, state_path: Path
    ) -> None:
        try:
            TypeAdapter(
                Batch.model_fields["source_id"].rebuild_annotation()
            ).validate_python(source_id)
        except ValidationError:
            raise ValueError("invalid connector source identity") from None
        self.source_id = source_id
        self.destination = destination
        self.state_path = state_path.resolve()
        self._lock = asyncio.Lock()
        self._tasks: set[asyncio.Task[Receipt | None]] = set()
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

    async def send(self, changes: Changes) -> Receipt:
        """Persist and deliver one batch. Failure leaves it saved for recovery.

        Retrying identical changes while that batch is pending reuses its receipt.
        Different changes wait for pending work to be acknowledged first.
        """
        if not changes.mutations:
            raise InvalidBatch("cannot send an empty batch")
        captured = Changes(
            tuple(item.model_copy(deep=True) for item in changes.mutations)
        )
        result = await self._call(lambda: self._send(captured))
        if result is None:
            raise RuntimeError("delivery returned no receipt")
        return result

    async def resume(self) -> Receipt | None:
        """Retry saved delivery, if any; normally performed automatically on open."""
        return await self._call(lambda: self._send(None))

    async def _call(self, operation: Callable[[], Receipt | None]) -> Receipt | None:
        if not self._entered:
            raise RuntimeError("use the connector inside an async context manager")

        async def serialized() -> Receipt | None:
            # The task owns the lock, so cancelling its caller cannot allow a
            # later send to race the still-running blocking delivery.
            async with self._lock:
                return await asyncio.to_thread(operation)

        task = asyncio.create_task(serialized())
        self._tasks.add(task)
        task.add_done_callback(self._finished)
        return await asyncio.shield(task)

    def _finished(self, task: asyncio.Task[Receipt | None]) -> None:
        self._tasks.discard(task)
        if not task.cancelled():
            task.exception()  # Consume errors even if its caller was cancelled.

    def _send(self, changes: Changes | None) -> Receipt | None:
        destination_id = self.destination._identity()
        try:
            with StateStore(self.state_path) as store:
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
                    receipt = self._deliver(store, pending)
                    if same or changes is None:
                        return receipt
                if changes is None:
                    return None
                return self._deliver(
                    store,
                    store.prepare(
                        changes, source_id=self.source_id, destination_id=destination_id
                    ),
                )
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

    def _deliver(self, store: StateStore, pending: PendingBatch) -> Receipt:
        payload = deliver(self.destination, pending.payload, self.state_path)
        return store.acknowledge(
            payload,
            source_id=self.source_id,
            destination_id=self.destination._identity(),
        )
