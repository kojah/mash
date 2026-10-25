"""Cancellation cannot close a registry still used by accepted blocking work."""

import asyncio
import tempfile
import threading
import unittest
from pathlib import Path

from mash_obsidian.connector import finish
from mash_obsidian.registry import Registry


class ShutdownTests(unittest.IsolatedAsyncioTestCase):
    async def test_cancel_waits_for_registry_work_before_cleanup(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with Registry(Path(directory), "test") as registry:
                started = threading.Event()
                release = threading.Event()

                def work() -> int:
                    started.set()
                    if not release.wait(5):
                        raise TimeoutError("test did not release accepted work")
                    return len(registry.active())

                task = asyncio.create_task(finish(work))
                await asyncio.to_thread(started.wait, 5)
                task.cancel()
                await asyncio.sleep(0)
                self.assertFalse(task.done())
                release.set()
                with self.assertRaises(asyncio.CancelledError):
                    await task
                self.assertEqual(registry.active(), [])
