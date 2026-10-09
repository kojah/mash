"""High-level delivery through the real Mash CLI, not a mock receipt backend."""

import asyncio
import os
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

from mash_sdk import (
    Changes,
    Connector,
    DeliveryError,
    Destination,
    KindRule,
    Mode,
    Role,
)
from mash_sdk._state import StateStore
from tests.objects import channel, message, note

# Tests exercise every operation, so their connector declares both modes.
BOTH = (Mode.snapshot, Mode.live)


class ConnectorTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        binary = os.environ.get("MASH_CONNECTOR_MASH_BINARY")
        if binary is None:
            self.skipTest("set MASH_CONNECTOR_MASH_BINARY for actual CLI delivery")
        if not Path(binary).is_file():
            self.fail("configured Mash binary is missing")
        self.binary = binary
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.database = self.root / "silver.sqlite3"
        self.state = self.root / "sdk.sqlite"
        self.config = self.root / "config.toml"
        self.config.write_text("version=1\n[providers]\n")

    def destination(self, *, executable: str | None = None) -> Destination:
        return Destination.local(
            database=self.database,
            executable=executable or self.binary,
            config=self.config,
        )

    def connector(self, destination: Destination) -> Connector:
        return Connector(
            source_id="external",
            modes=BOTH,
            destinations=[destination],
            state_path=self.state,
        )

    def wrapper(self, *, stall: bool = False, gated: bool = False) -> str:
        if os.name != "posix":
            self.skipTest("fault-injection executable fixture requires POSIX")
        path = self.root / "delivery-wrapper"
        marker = self.root / "response-dropped"
        script = (
            f"#!{sys.executable}\n"
            "import subprocess, sys, time\nfrom pathlib import Path\n"
        )
        if stall:
            script += (
                "print('PRIVATE-CANARY', file=sys.stderr, flush=True)\ntime.sleep(10)\n"
            )
        elif gated:
            script += (
                f"Path({str(self.root / 'started')!r}).touch()\n"
                f"while not Path({str(self.root / 'release')!r}).exists():\n"
                "    time.sleep(0.01)\n"
                f"sys.exit(subprocess.call([{self.binary!r}, *sys.argv[1:]]))\n"
            )
        else:
            script += (
                f"result = subprocess.run([{self.binary!r}, *sys.argv[1:]], "
                "stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)\n"
                f"marker = Path({str(marker)!r})\n"
                "if marker.exists():\n    sys.stdout.buffer.write(result.stdout)\n"
                "else:\n    marker.touch()\n"
                "    sys.stdout.buffer.write(b'{lost response')\n"
                "sys.exit(result.returncode)\n"
            )
        path.write_text(script)
        path.chmod(0o700)
        return str(path)

    async def test_concurrent_sends_are_ordered_and_use_actual_silver_receipts(
        self,
    ) -> None:
        async with self.connector(self.destination()) as connector:
            receipts = await asyncio.gather(
                connector.send(Changes.upsert(note(id="one", content="First"))),
                connector.send(Changes.upsert(note(id="two", content="Second"))),
            )
        receipts = [receipt for (receipt,) in receipts]
        self.assertEqual([item.source_revision for item in receipts], [1, 2])
        self.assertTrue(all(not item.gold_ready for item in receipts))
        with sqlite3.connect(self.database) as connection:
            self.assertEqual(
                connection.execute(
                    "SELECT count(*) FROM objects"
                    " WHERE origin='connector' AND object_role='document'"
                ).fetchone()[0],
                2,
            )
        self.assertEqual(list(self.root.glob(".mash-delivery-*")), [])

    async def test_lost_response_and_same_changes_retry_reuse_the_saved_batch(
        self,
    ) -> None:
        destination = self.destination(executable=self.wrapper())
        changes = Changes.upsert(note(id="note", content="A durable note"))
        async with self.connector(destination) as connector:
            with self.assertRaises(DeliveryError):
                await connector.send(changes)
            with StateStore(self.state) as store:
                pending = store.pending(
                    source_id="external", destination_id=destination._identity()
                )
                self.assertIsNotNone(pending)
            (receipt,) = await connector.send(changes)
        self.assertEqual(receipt.source_revision, 1)
        assert pending is not None
        self.assertEqual(receipt.batch_id, pending.batch_id)
        with sqlite3.connect(self.database) as connection:
            self.assertEqual(
                connection.execute(
                    "SELECT count(*) FROM connector_receipts"
                ).fetchone()[0],
                1,
            )

    async def test_reopening_automatically_recovers_saved_delivery(self) -> None:
        destination = self.destination(executable=self.wrapper())
        async with self.connector(destination) as first:
            with self.assertRaises(DeliveryError):
                await first.send(Changes.upsert(note(id="note", content="Restart")))
        async with self.connector(destination):
            pass
        with StateStore(self.state) as store:
            checkpoint = store.checkpoint(
                source_id="external", destination_id=destination._identity()
            )
            self.assertEqual(checkpoint.revision, 1)
            self.assertIsNone(
                store.pending(
                    source_id="external", destination_id=destination._identity()
                )
            )

    async def test_timeout_keeps_changes_saved_and_does_not_expose_process_output(
        self,
    ) -> None:
        destination = Destination.local(
            database=self.database,
            executable=self.wrapper(stall=True),
            timeout=0.05,
        )
        async with self.connector(destination) as connector:
            with self.assertRaises(DeliveryError) as raised:
                await connector.send(
                    Changes.upsert(note(id="note", content="PRIVATE-CANARY"))
                )
            self.assertTrue(raised.exception.retryable)
            self.assertNotIn("PRIVATE-CANARY", str(raised.exception))
        with StateStore(self.state) as store:
            self.assertIsNotNone(
                store.pending(
                    source_id="external", destination_id=destination._identity()
                )
            )
        self.assertEqual(list(self.root.glob(".mash-delivery-*")), [])

    async def test_cancelling_a_caller_preserves_delivery_and_serializes_the_next_send(
        self,
    ) -> None:
        destination = self.destination(executable=self.wrapper(gated=True))

        async def started() -> None:
            while not (self.root / "started").exists():
                await asyncio.sleep(0.01)

        async with self.connector(destination) as connector:
            caller = asyncio.create_task(
                connector.send(
                    Changes.upsert(
                        note(id="one", content="Accepted before cancellation")
                    )
                )
            )
            await asyncio.wait_for(started(), timeout=10)
            caller.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await caller
            later = asyncio.create_task(
                connector.send(
                    Changes.upsert(note(id="two", content="Sent after cancellation"))
                )
            )
            (self.root / "release").touch()
            (receipt,) = await later
            self.assertEqual(receipt.source_revision, 2)
        with sqlite3.connect(self.database) as connection:
            self.assertEqual(
                connection.execute(
                    "SELECT count(*) FROM objects"
                    " WHERE origin='connector' AND object_role='document'"
                ).fetchone()[0],
                2,
            )

    async def test_declared_schema_is_enforced_by_mash(self) -> None:
        schema = Changes.declare(
            KindRule(kind="chat:channel", role=Role.container, root=True),
            KindRule(kind="chat:message", role=Role.entry, parents=["chat:channel"]),
        )
        async with self.connector(self.destination()) as connector:
            await connector.send(
                schema
                + Changes.upsert(
                    channel(id="room"),
                    message(id="hello", parent="room", content="Hello"),
                )
            )
            with self.assertRaises(DeliveryError) as raised:
                await connector.send(Changes.upsert(note(id="note", content="x")))
        self.assertFalse(raised.exception.retryable)
        with sqlite3.connect(self.database) as connection:
            stored = connection.execute(
                "SELECT count(*) FROM objects"
                " WHERE origin='connector' AND object_role='document'"
            ).fetchone()[0]
        self.assertEqual(stored, 0)


class ConnectorConfigurationTests(unittest.TestCase):
    def test_invalid_source_and_timeout_are_rejected_before_state_creation(
        self,
    ) -> None:
        destination = Destination.local(database=Path("unused.sqlite"))
        with self.assertRaises(ValueError):
            Connector(
                source_id="invalid source",
                modes=BOTH,
                destinations=[destination],
                state_path=Path("unused-state.sqlite"),
            )
        for timeout in [0, -1, float("inf"), float("nan")]:
            with self.assertRaises(ValueError):
                Destination.local(database=Path("unused.sqlite"), timeout=timeout)
