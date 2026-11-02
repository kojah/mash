"""Connector.run: backfill diffs, captured events, debounce and recovery."""

import asyncio
import os
import sqlite3
import tempfile
import unittest
from collections.abc import AsyncIterator, Awaitable, Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path

import orjson

from mash_sdk import (
    Changes,
    Connector,
    Debounce,
    Delivery,
    DeliveryError,
    Destination,
    DocumentSnapshot,
    Mode,
    Object,
    Snapshot,
)
from tests.fake_mash import (
    STATE_DIR_ENV,
    TOKEN,
    TOKEN_ENV,
    FileBehaviour,
    install_executable,
)
from tests.objects import channel, message, note

# Tests exercise every operation, so their connector declares both modes.
BOTH = (Mode.snapshot, Mode.live)

SINCE = datetime(2026, 1, 1, tzinfo=UTC)
BEFORE = int(SINCE.timestamp() * 1000) - 1
AFTER = int(SINCE.timestamp() * 1000) + 1
QUICK = Debounce(quiet_seconds=0.05, max_wait_seconds=1)


@dataclass(frozen=True)
class Event:
    id: str
    version: int


class Events:
    """A live event source the test feeds and closes."""

    def __init__(self) -> None:
        self.queue: asyncio.Queue[Delivery[Event] | None] = asyncio.Queue()

    def put(
        self, event: Event, acknowledge: Callable[[], Awaitable[None]] | None = None
    ) -> None:
        self.queue.put_nowait(
            Delivery(
                id=f"{event.id}:{event.version}",
                key=event.id,
                value=event,
                acknowledge=acknowledge,
            )
        )

    def close(self) -> None:
        self.queue.put_nowait(None)

    async def __aiter__(self) -> AsyncIterator[Delivery[Event]]:
        while (delivery := await self.queue.get()) is not None:
            yield delivery


def closed() -> Events:
    events = Events()
    events.close()
    return events


async def snapshots(
    *items: Snapshot | DocumentSnapshot,
) -> AsyncIterator[Snapshot | DocumentSnapshot]:
    for item in items:
        yield item


def entry(
    id: str, created_at: int | None = AFTER, reply_to: str | None = None
) -> Object:
    return message(
        id=id,
        parent="chat",
        content=f"text {id}",
        created_at=created_at,
        reply_to=reply_to,
    )


def chat_snapshot(*entries: Object, complete: bool = True) -> Snapshot:
    return Snapshot(
        container=channel(id="chat", title="Chat"),
        objects=entries,
        complete=complete,
        created_since=SINCE,
    )


def documents(
    *ids: str, scopes: tuple[str, ...], complete: bool = True
) -> DocumentSnapshot:
    return DocumentSnapshot(
        documents=[note(id=id, content=f"note {id}") for id in ids],
        scope_ids=scopes,
        complete=complete,
    )


async def unexpected(event: Event) -> Changes:
    raise AssertionError("no events were expected")


class Harness(unittest.IsolatedAsyncioTestCase):
    """A fake shared destination and a connector state file; no tests itself."""

    async def asyncSetUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.state_path = Path(temporary.name) / "sdk.sqlite"
        self.server = FileBehaviour(Path(temporary.name), "http://127.0.0.1:8787")
        os.environ[STATE_DIR_ENV] = temporary.name
        self.addCleanup(os.environ.pop, STATE_DIR_ENV, None)
        os.environ["MASH_EXECUTABLE"] = str(install_executable(Path(temporary.name)))
        self.addCleanup(os.environ.pop, "MASH_EXECUTABLE", None)
        self.destination = Destination.shared(
            api_url="http://127.0.0.1:8787",
            group_id="group",
            node_id="laptop",
            token_env=TOKEN_ENV,
        )
        os.environ[TOKEN_ENV] = TOKEN
        self.addCleanup(os.environ.pop, TOKEN_ENV, None)

    def connector(self) -> Connector:
        return Connector(
            source_id="external",
            modes=BOTH,
            destinations=[self.destination],
            state_path=self.state_path,
        )

    async def run_once(
        self,
        *items: Snapshot | DocumentSnapshot,
        events: Events | None = None,
        refresh: Callable[[Event], Awaitable[Changes]] = unexpected,
        debounce: Debounce = QUICK,
    ) -> None:
        async with self.connector() as connector:
            await asyncio.wait_for(
                connector.run(
                    backfill=snapshots(*items),
                    events=events or closed(),
                    event_type=Event,
                    refresh=refresh,
                    debounce=debounce,
                ),
                timeout=30,
            )

    def journal(self) -> list[tuple[str, int]]:
        with sqlite3.connect(self.state_path) as connection:
            return connection.execute("SELECT key,sequence FROM events").fetchall()


class RunnerTests(Harness):
    async def test_unchanged_backfill_is_not_resent(self) -> None:
        snapshot = chat_snapshot(entry("one"), entry("two"))
        await self.run_once(snapshot)
        await self.run_once(snapshot)
        self.assertEqual(len(self.server.commits), 1)
        await self.run_once(chat_snapshot(entry("one"), entry("two", AFTER + 1)))
        self.assertEqual(len(self.server.commits), 2)
        self.assertEqual(self.server.messages["two"]["created_at"], AFTER + 1)

    async def test_complete_chat_snapshot_deletes_only_omitted_messages_in_window(
        self,
    ) -> None:
        await self.run_once(
            chat_snapshot(
                entry("old", BEFORE),
                entry("unknown", None),
                entry("kept"),
                entry("gone"),
                entry("parent"),
                complete=False,
            )
        )
        # An incomplete snapshot never turns absence into deletion.
        await self.run_once(chat_snapshot(complete=False))
        self.assertEqual(len(self.server.messages), 5)
        await self.run_once(
            chat_snapshot(entry("kept"), entry("child", reply_to="parent"))
        )
        self.assertEqual(
            set(self.server.messages), {"old", "unknown", "kept", "parent", "child"}
        )

    async def test_document_omission_needs_a_complete_snapshot_covering_its_scope(
        self,
    ) -> None:
        await self.run_once(documents("a", "b", scopes=("A", "B")))
        async with self.connector() as connector:
            await connector.send(Changes.upsert(note(id="sent", content="direct")))
        # A narrower scope does not cover documents delivered from (A, B).
        await self.run_once(documents(scopes=("A",)))
        self.assertEqual(set(self.server.documents), {"a", "b", "sent"})
        await self.run_once(documents("a", scopes=("A", "B")))
        # Documents never delivered by a complete snapshot are not omitted.
        self.assertEqual(set(self.server.documents), {"a", "sent"})
        # An unchanged document adopts the scope of the latest snapshot holding it.
        await self.run_once(documents("a", scopes=("A",)))
        await self.run_once(documents(scopes=("A",)))
        self.assertEqual(set(self.server.documents), {"sent"})

    async def test_events_are_journaled_before_capture_ack_and_coalesce_after_backfill(
        self,
    ) -> None:
        events = Events()
        released = asyncio.Event()
        journaled: list[list[tuple[str, int]]] = []
        refreshed: list[Event] = []

        async def acknowledge() -> None:
            journaled.append(self.journal())

        async def gated() -> AsyncIterator[Snapshot | DocumentSnapshot]:
            await released.wait()
            yield documents("note", scopes=("notes",))

        async def refresh(event: Event) -> Changes:
            refreshed.append(event)
            return Changes.upsert(note(id="note", content=f"v{event.version}"))

        async with self.connector() as connector:
            running = asyncio.create_task(
                connector.run(
                    backfill=gated(),
                    events=events,
                    event_type=Event,
                    refresh=refresh,
                    debounce=QUICK,
                )
            )
            for version in (1, 2, 3):
                events.put(Event("note", version), acknowledge)
            while len(journaled) < 3:
                await asyncio.sleep(0.01)
            await asyncio.sleep(0.2)  # Well past quiet_seconds.
            self.assertEqual(refreshed, [])
            released.set()
            events.close()
            await asyncio.wait_for(running, timeout=30)
        self.assertEqual(journaled, [[("note", 0)], [("note", 1)], [("note", 2)]])
        self.assertEqual(refreshed, [Event("note", 3)])
        self.assertEqual(self.server.documents["note"]["content"], "v3")
        self.assertEqual(self.journal(), [])

    async def test_max_wait_refreshes_a_key_that_never_goes_quiet(self) -> None:
        events = Events()
        refreshed = asyncio.Event()

        async def refresh(event: Event) -> Changes:
            refreshed.set()
            return Changes.empty()

        async with self.connector() as connector:
            running = asyncio.create_task(
                connector.run(
                    backfill=snapshots(),
                    events=events,
                    event_type=Event,
                    refresh=refresh,
                    # Events arrive every 20ms, so the key is never quiet.
                    debounce=Debounce(quiet_seconds=0.3, max_wait_seconds=0.3),
                )
            )
            version = 0
            while not refreshed.is_set():
                version += 1
                events.put(Event("busy", version))
                await asyncio.sleep(0.02)
                self.assertLess(version, 250, "max_wait did not trigger a refresh")
            events.close()
            await asyncio.wait_for(running, timeout=30)

    async def test_failed_refresh_keeps_the_event_for_the_next_run(self) -> None:
        events = Events()
        events.put(Event("note", 1))
        events.close()

        async def broken(event: Event) -> Changes:
            raise RuntimeError("source unavailable")

        with self.assertRaises(RuntimeError):
            await self.run_once(events=events, refresh=broken)
        self.assertEqual(self.journal(), [("note", 0)])
        refreshed: list[Event] = []

        async def refresh(event: Event) -> Changes:
            refreshed.append(event)
            return Changes.upsert(note(id="note", content="recovered"))

        await self.run_once(refresh=refresh)
        self.assertEqual(refreshed, [Event("note", 1)])
        self.assertEqual(self.server.documents["note"]["content"], "recovered")
        self.assertEqual(self.journal(), [])

    async def test_retryable_failures_back_off_and_permanent_ones_end_the_run(
        self,
    ) -> None:
        self.server.failures = [503]
        await self.run_once(documents("a", scopes=("A",)))
        self.assertEqual(set(self.server.documents), {"a"})
        self.server.failures = [409]
        with self.assertRaises(DeliveryError) as raised:
            await self.run_once(documents("a", "b", scopes=("A",)))
        self.assertFalse(raised.exception.retryable)

    async def test_large_snapshots_are_split_under_the_batch_limits(self) -> None:
        await self.run_once(chat_snapshot(*(entry(f"m{i}") for i in range(600))))
        self.assertEqual(len(self.server.commits), 2)
        self.assertEqual(len(self.server.messages), 600)
        self.assertIn("chat", self.server.chats)

    async def test_snapshot_messages_must_belong_to_the_chat(self) -> None:
        with self.assertRaises(ValueError):
            Snapshot(
                container=channel(id="chat"),
                objects=[message(id="m", parent="other", content="x")],
                complete=True,
            )


class SyncTests(Harness):
    """`sync` publishes polled snapshots with backfill's rules, without events."""

    async def test_polling_resends_only_changes_and_deletes_omissions(self) -> None:
        async with self.connector() as connector:
            self.assertEqual(
                await connector.sync(chat_snapshot(entry("one"), entry("two"))), 1
            )
            self.assertEqual(
                await connector.sync(chat_snapshot(entry("one"), entry("two"))), 0
            )
            await connector.sync(chat_snapshot(entry("one")))
        self.assertEqual(set(self.server.messages), {"one"})
        self.assertEqual(len(self.server.commits), 2)

    async def test_failures_raise_and_the_next_poll_resumes(self) -> None:
        self.server.failures = [503]
        async with self.connector() as connector:
            with self.assertRaises(DeliveryError) as raised:
                await connector.sync(documents("a", scopes=("A",)))
            self.assertTrue(raised.exception.retryable)
            await connector.sync(documents("a", scopes=("A",)))
        self.assertEqual(set(self.server.documents), {"a"})

    async def test_sync_needs_an_open_connector(self) -> None:
        with self.assertRaises(RuntimeError):
            await self.connector().sync(documents("a", scopes=("A",)))


class ModeTests(Harness):
    """Declared modes gate operations, and each batch says what produced it."""

    def triggers(self) -> list[tuple[str, list[str]]]:
        batches = [orjson.loads(body) for body in self.server.commits]
        return [(batch["trigger"], batch["modes"]) for batch in batches]

    async def test_batches_record_what_produced_them(self) -> None:
        async with self.connector() as connector:
            await connector.sync(chat_snapshot(entry("one"), complete=False))
            await connector.sync(chat_snapshot(entry("one"), entry("two")))
            await connector.send(Changes.upsert(note(id="manual", content="text")))
        both = ["live", "snapshot"]
        self.assertEqual(
            self.triggers(),
            [("snapshot", both), ("complete-snapshot", both), ("change", both)],
        )

    async def test_live_refreshes_are_marked_live(self) -> None:
        events = Events()
        events.put(Event("note", 1))
        events.close()

        async def refresh(event: Event) -> Changes:
            return Changes.upsert(note(id=event.id, content="refreshed"))

        await self.run_once(
            documents("a", scopes=("A",)), events=events, refresh=refresh
        )
        self.assertEqual(
            [trigger for trigger, _ in self.triggers()], ["complete-snapshot", "live"]
        )

    async def test_operations_need_their_declared_mode(self) -> None:
        def connector(*modes: Mode) -> Connector:
            return Connector(
                source_id="external",
                modes=modes,
                destinations=[self.destination],
                state_path=self.state_path,
            )

        with self.assertRaises(ValueError):
            connector()
        async with connector(Mode.live) as live:
            with self.assertRaises(ValueError):
                await live.sync(documents("a", scopes=("A",)))
        async with connector(Mode.snapshot) as polled:
            with self.assertRaises(ValueError):
                await polled.run(
                    backfill=snapshots(),
                    events=closed(),
                    event_type=Event,
                    refresh=unexpected,
                    debounce=QUICK,
                )
        self.assertEqual(self.server.commits, [])


class RealMashRunnerTests(unittest.IsolatedAsyncioTestCase):
    async def test_snapshot_deletion_is_accepted_by_local_mash(self) -> None:
        binary = os.environ.get("MASH_CONNECTOR_MASH_BINARY")
        if binary is None:
            self.skipTest("set MASH_CONNECTOR_MASH_BINARY for actual CLI delivery")
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        config = root / "config.toml"
        config.write_text("version=1\n[providers]\n")
        database = root / "silver.sqlite3"
        destination = Destination.local(
            database=database, executable=binary, config=config
        )
        for snapshot in (
            chat_snapshot(entry("kept"), entry("gone")),
            chat_snapshot(entry("kept"), entry("child", reply_to="kept")),
        ):
            async with Connector(
                source_id="external",
                modes=BOTH,
                destinations=[destination],
                state_path=root / "sdk.sqlite",
            ) as connector:
                await connector.run(
                    backfill=snapshots(snapshot),
                    events=closed(),
                    event_type=Event,
                    refresh=unexpected,
                    debounce=QUICK,
                )
        with sqlite3.connect(database) as connection:
            stored = {
                row[0]
                for row in connection.execute(
                    "SELECT id FROM objects"
                    " WHERE origin='connector' AND object_role='entry'"
                )
            }
        self.assertEqual(stored, {"kept", "child"})


if __name__ == "__main__":
    unittest.main()


class FanOutTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.state_path = Path(temporary.name) / "sdk.sqlite"
        urls = ("http://127.0.0.1:8787", "http://127.0.0.1:8788")
        self.servers = tuple(
            FileBehaviour(Path(temporary.name), url) for url in urls
        )
        os.environ[STATE_DIR_ENV] = temporary.name
        self.addCleanup(os.environ.pop, STATE_DIR_ENV, None)
        os.environ["MASH_EXECUTABLE"] = str(install_executable(Path(temporary.name)))
        self.addCleanup(os.environ.pop, "MASH_EXECUTABLE", None)
        self.destinations = [
            Destination.shared(
                api_url=url,
                group_id="group",
                node_id="laptop",
                token_env=TOKEN_ENV,
                timeout=5,
            )
            for url in urls
        ]
        os.environ[TOKEN_ENV] = TOKEN
        self.addCleanup(os.environ.pop, TOKEN_ENV, None)

    def connector(self) -> Connector:
        return Connector(
            source_id="external",
            modes=BOTH,
            destinations=self.destinations,
            state_path=self.state_path,
        )

    async def test_each_destination_receives_and_acknowledges_independently(
        self,
    ) -> None:
        healthy, flaky = self.servers
        flaky.failures = [503]
        async with self.connector() as connector:
            with self.assertRaises(DeliveryError) as raised:
                await connector.send(Changes.upsert(note(id="a", content="one")))
            self.assertTrue(raised.exception.retryable)
            self.assertIn("1 of 2", str(raised.exception))
            self.assertEqual(set(healthy.documents), {"a"})
            self.assertEqual(flaky.documents, {})
            # The flaky destination's saved batch goes first, then the new one.
            receipts = await connector.send(Changes.upsert(note(id="b", content="two")))
        self.assertEqual([receipt.source_revision for receipt in receipts], [2, 2])
        for server in self.servers:
            self.assertEqual(set(server.documents), {"a", "b"})

    async def test_one_refresh_serves_every_destination_and_permanent_failures_end_runs(
        self,
    ) -> None:
        events = Events()
        events.put(Event("note", 1))
        events.close()
        refreshed: list[Event] = []

        async def refresh(event: Event) -> Changes:
            refreshed.append(event)
            return Changes.upsert(note(id="note", content="current"))

        async with self.connector() as connector:
            await connector.run(
                backfill=snapshots(documents("a", scopes=("A",))),
                events=events,
                event_type=Event,
                refresh=refresh,
                debounce=QUICK,
            )
        self.assertEqual(refreshed, [Event("note", 1)])
        for server in self.servers:
            self.assertEqual(set(server.documents), {"a", "note"})
        self.servers[1].failures = [409]
        with self.assertRaises(DeliveryError) as raised:
            async with self.connector() as connector:
                await connector.run(
                    backfill=snapshots(documents("a", "b", scopes=("A",))),
                    events=closed(),
                    event_type=Event,
                    refresh=unexpected,
                    debounce=QUICK,
                )
        self.assertFalse(raised.exception.retryable)
        self.assertEqual(set(self.servers[0].documents), {"a", "b", "note"})

    def test_destinations_must_be_present_and_distinct(self) -> None:
        for destinations in ([], [self.destinations[0], self.destinations[0]]):
            with self.assertRaises(ValueError):
                Connector(
                    source_id="external",
                    modes=BOTH,
                    destinations=destinations,
                    state_path=self.state_path,
                )
