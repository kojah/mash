"""Shared delivery through the mash executable, without a live server."""

import dataclasses
import os
import tempfile
import unittest
from pathlib import Path

from mash_sdk import Changes, Connector, DeliveryError, Destination, Mode
from mash_sdk._state import StateStore
from tests.fake_mash import STATE_DIR_ENV, FileBehaviour, install_executable
from tests.objects import note

BOTH = (Mode.snapshot, Mode.live)
TOKEN = "test-node-token-32-bytes-long-abc"
TOKEN_ENV = "MASH_TEST_NODE_TOKEN"


# Emulates `mash index --connector-batch <file> --shared`: reads the batch,
# prints the receipt the SDK validates, and fails on demand.
def edit(content: str) -> Changes:
    return Changes.upsert(note(id="note", content=content))


class SharedDeliveryTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.state_path = root / "sdk.sqlite"
        self.executable = install_executable(root)
        # The fake derives its state file from the destination's api_url.
        self.server = FileBehaviour(root, "http://127.0.0.1:8787")
        os.environ["MASH_EXECUTABLE"] = str(self.executable)
        self.addCleanup(os.environ.pop, "MASH_EXECUTABLE", None)
        os.environ[STATE_DIR_ENV] = str(root)
        self.addCleanup(os.environ.pop, STATE_DIR_ENV, None)
        os.environ[TOKEN_ENV] = TOKEN
        self.addCleanup(os.environ.pop, TOKEN_ENV, None)
        self.addCleanup(os.environ.pop, "FAKE_MASH_EXIT", None)
        self.addCleanup(os.environ.pop, "FAKE_MASH_SLEEP", None)

    def destination(self) -> Destination:
        return Destination.shared(
            api_url="http://127.0.0.1:8787",
            group_id="group",
            node_id="laptop",
            token_env=TOKEN_ENV,
            timeout=10,
        )

    def connector(self) -> Connector:
        return Connector(
            source_id="external",
            modes=BOTH,
            destinations=[self.destination()],
            state_path=self.state_path,
        )

    def pending(self, destination: Destination) -> bool:
        with StateStore(self.state_path) as store:
            return (
                store.pending(
                    source_id="external", destination_id=destination.identity()
                )
                is not None
            )

    async def test_delivers_batches_in_source_order(self) -> None:
        async with self.connector() as connector:
            (first,) = await connector.send(edit("one"))
            (second,) = await connector.send(edit("two"))
        self.assertEqual((first.source_revision, second.source_revision), (1, 2))
        self.assertFalse(self.pending(self.destination()))

    async def test_capacity_failure_is_retryable_and_keeps_the_batch(self) -> None:
        os.environ["FAKE_MASH_EXIT"] = "6"
        destination = self.destination()
        with self.assertRaises(DeliveryError) as raised:
            async with self.connector() as connector:
                await connector.send(edit("one"))
        self.assertTrue(raised.exception.retryable)
        self.assertTrue(self.pending(destination))

    async def test_permanent_failure_is_not_retryable(self) -> None:
        os.environ["FAKE_MASH_EXIT"] = "2"
        with self.assertRaises(DeliveryError) as raised:
            async with self.connector() as connector:
                await connector.send(edit("one"))
        self.assertFalse(raised.exception.retryable)

    async def test_stalled_executable_times_out_as_retryable(self) -> None:
        os.environ["FAKE_MASH_SLEEP"] = "5"
        with self.assertRaises(DeliveryError) as raised:
            async with Connector(
                source_id="external",
                modes=BOTH,
                destinations=[dataclasses.replace(self.destination(), timeout=0.5)],
                state_path=self.state_path,
            ) as connector:
                await connector.send(edit("one"))
        self.assertTrue(raised.exception.retryable)

    async def test_missing_token_fails_before_running_mash(self) -> None:
        os.environ.pop(TOKEN_ENV)
        with self.assertRaises(DeliveryError) as raised:
            async with self.connector() as connector:
                await connector.send(edit("one"))
        self.assertFalse(raised.exception.retryable)
