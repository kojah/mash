"""One live connector per source key, across processes."""

import os
import tempfile
import unittest
from pathlib import Path

from mash_sdk import Connector, Destination, Mode
from mash_sdk.connector import SourceInUse


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
