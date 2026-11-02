"""One live connector per source key, across processes."""

import os
import tempfile
import unittest
from pathlib import Path

from mash_sdk import Connector, Destination, Mode
from mash_sdk.connector import DestinationNodes, SourceInUse


class SourceLockTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        os.environ["MASH_CONNECTOR_LOCKS"] = str(self.root / "locks")
        self.addCleanup(os.environ.pop, "MASH_CONNECTOR_LOCKS", None)

    def connector(self, source: str, name: str, share: bool = False) -> Connector:
        return Connector(
            source_id=source,
            modes=[Mode.snapshot],
            destinations=[Destination.local(database=self.root / f"{name}.sqlite3")],
            state_path=self.root / f"{name}-state.sqlite3",
            share_state=share,
        )

    async def test_a_second_instance_of_one_source_is_rejected(self) -> None:
        first = self.connector("external", "one")
        second = self.connector("external", "two")
        async with first:
            with self.assertRaises(SourceInUse):
                await second.__aenter__()

    async def test_distinct_sources_open_together(self) -> None:
        one = self.connector("slack:T:U1", "one")
        two = self.connector("git:host/owner/repo", "two")
        async with one, two:
            pass

    async def test_sharing_state_is_explicit(self) -> None:
        one = self.connector("external", "one", share=True)
        two = self.connector("external", "two", share=True)
        async with one, two:
            pass

    async def test_the_lock_is_released_on_exit(self) -> None:
        async with self.connector("external", "one"):
            pass
        async with self.connector("external", "two"):
            pass

    async def test_device_sources_carry_the_node(self) -> None:
        import socket

        device = Connector(
            source_id="obsidian:vault",
            modes=[Mode.snapshot],
            destinations=[Destination.local(database=self.root / "v.sqlite3")],
            state_path=self.root / "v-state.sqlite3",
            device_scope=True,
        )
        async with device:
            self.assertEqual(device.source_id, f"obsidian:vault:{socket.gethostname()}")

    async def test_device_sources_reject_disagreeing_nodes(self) -> None:
        first = Destination.shared(
            api_url="http://127.0.0.1:8787",
            group_id="g",
            node_id="laptop",
            token_env="T",
        )
        second = Destination.shared(
            api_url="http://127.0.0.1:8788",
            group_id="g",
            node_id="server",
            token_env="T",
        )
        device = Connector(
            source_id="imessage:account",
            modes=[Mode.snapshot],
            destinations=[first, second],
            state_path=self.root / "i-state.sqlite3",
            device_scope=True,
        )
        with self.assertRaises(DestinationNodes):
            await device.__aenter__()

    async def test_account_sources_keep_the_node_out_of_the_key(self) -> None:
        account = Connector(
            source_id="slack:T0TEAM:U0ALICE",
            modes=[Mode.snapshot],
            destinations=[Destination.local(database=self.root / "a.sqlite3")],
            state_path=self.root / "a-state.sqlite3",
        )
        async with account:
            self.assertEqual(account.source_id, "slack:T0TEAM:U0ALICE")
