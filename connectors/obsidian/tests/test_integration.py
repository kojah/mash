"""Real CLI silver delivery, replay and searchable evidence; no models required."""

import asyncio
import os
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

import orjson
from mash_sdk import DeliveryError

from mash_obsidian.config import Local, Settings
from mash_obsidian.connector import Result, run


class IntegrationTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        binary = os.environ.get("MASH_CONNECTOR_MASH_BINARY")
        if binary is None:
            self.skipTest(
                "set MASH_CONNECTOR_MASH_BINARY for real Mash delivery/search"
            )
        self.binary = str(Path(binary).resolve())
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.vault = self.root / "vault"
        self.vault.mkdir()
        self.database = self.root / "silver.sqlite3"
        self.config = self.root / "mash.toml"
        self.config.write_text(
            "version=1\n[providers]\n[views.build]\n"
            "representations='lexical-only'\n[views.query]\nintent='exact-mention'\n"
        )
        self.settings = Settings(
            vault=self.vault,
            vault_id="test",
            state_dir=self.root / "state",
            folders=(".",),
            deletion_grace_seconds=0,
            destinations=(
                Local(
                    type="local",
                    database=self.database,
                    executable=self.binary,
                    config=self.config,
                ),
            ),
        )

    async def synchronize(self, settings: Settings | None = None) -> Result:
        reports: list[Result] = []
        await run(settings or self.settings, report=reports.append)
        return reports[-1]

    def notes(self) -> list[tuple[str, str, str]]:
        with sqlite3.connect(self.database) as connection:
            return connection.execute(
                "SELECT id,title,content FROM objects WHERE origin='connector' "
                "AND kind='obsidian:note' ORDER BY id"
            ).fetchall()

    def mash(self, *arguments: str) -> dict:
        output = subprocess.run(
            [
                self.binary,
                "--config",
                str(self.config),
                "--db",
                str(self.database),
                "--models-dir",
                str(self.root / "no-models"),
                *arguments,
            ],
            capture_output=True,
            check=True,
        )
        return orjson.loads(output.stdout)

    async def test_notes_are_searchable_and_keep_identity_through_edit_and_rename(
        self,
    ) -> None:
        note = self.vault / "note.md"
        text = "---\ntitle: Architecture\n---\nA zephyrlighthouse needs silver."
        note.write_text(text)
        first = await self.synchronize()
        self.assertEqual(first.selected_notes, 1)
        self.assertEqual(first.delivered_batches, 1)
        original = self.notes()[0][0]
        self.mash("index")
        result = self.mash(
            "search",
            "zephyrlighthouse",
            "--local",
            "--source",
            "obsidian:test",
            "--intent",
            "exact-mention",
            "--unit",
            "messages",
        )
        self.assertTrue(result["results"])
        viewed = self.mash("view", result["results"][0]["lookup"]["view"]["id"])
        self.assertIn(text, viewed["messages"][0]["content"])
        self.assertEqual(self.notes()[0][2], text)
        self.assertFalse((self.root / "no-models").exists())
        self.assertEqual((await self.synchronize()).delivered_batches, 0)
        renamed = self.vault / "renamed.md"
        note.rename(renamed)
        await self.synchronize()
        self.assertEqual(self.notes()[0][0], original)
        temporary = self.vault / "atomic-save"
        temporary.write_text("# Updated\nA new zephyrharbor.")
        os.replace(temporary, renamed)
        await self.synchronize()
        self.assertEqual(self.notes()[0][0], original)
        self.mash("index", "--rebuild-local-views")
        self.assertTrue(self.mash("search", "zephyrharbor", "--local")["results"])
        renamed.unlink()
        self.assertEqual((await self.synchronize()).deleted_notes, 0)
        self.assertEqual(len(self.notes()), 1)
        self.assertEqual((await self.synchronize()).deleted_notes, 1)
        self.assertEqual(self.notes(), [])
        self.mash("index", "--rebuild-local-views")
        self.assertEqual(self.mash("search", "zephyrharbor", "--local")["results"], [])

    async def test_committed_delivery_with_lost_receipt_replays_after_restart(
        self,
    ) -> None:
        (self.vault / "note.md").write_text("Persisted source evidence")
        wrapper = self.root / "lose-receipt"
        wrapper.write_text(
            f"#!{sys.executable}\nimport pathlib, subprocess, sys\n"
            f"marker = pathlib.Path({str(self.root / 'lost')!r})\n"
            f"command = [{self.binary!r}, *sys.argv[1:]]\n"
            "result = subprocess.run(command, capture_output=True)\n"
            "if marker.exists():\n sys.stdout.buffer.write(result.stdout)\n"
            "else:\n marker.touch()\n"
            "sys.exit(result.returncode)\n"
        )
        wrapper.chmod(0o700)
        first = self.settings.destinations[0]
        assert isinstance(first, Local)
        failing = self.settings.model_copy(
            update={
                "destinations": (first.model_copy(update={"executable": str(wrapper)}),)
            }
        )
        with self.assertRaises(DeliveryError):
            await self.synchronize(failing)
        original = self.notes()[0][0]
        await self.synchronize()
        self.assertEqual(self.notes()[0][0], original)
        with sqlite3.connect(self.database) as connection:
            self.assertEqual(
                connection.execute(
                    "SELECT count(*) FROM connector_receipts"
                ).fetchone()[0],
                1,
            )

    async def test_watch_polls_edits_and_confirmed_deletions(self) -> None:
        note = self.vault / "note.md"
        note.write_text("Before")
        config = self.root / "obsidian.toml"
        config.write_text(
            f"vault={str(self.vault)!r}\nvault_id='test'\n"
            f"state_dir={str(self.settings.state_dir)!r}\nfolders=['.']\n"
            "poll_seconds=0.1\ndeletion_grace_seconds=0.1\n"
            "[[destinations]]\ntype='local'\n"
            f"database={str(self.database)!r}\nexecutable={self.binary!r}\n"
            f"config={str(self.config)!r}\n"
        )
        process = subprocess.Popen(
            [
                str(Path(sys.executable).parent / "mash-obsidian"),
                "--config",
                str(config),
                "--watch",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )

        async def until(predicate) -> None:
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    self.fail("polling connector exited unexpectedly")
                if self.database.exists():
                    try:
                        if predicate(self.notes()):
                            return
                    except sqlite3.OperationalError:
                        pass
                await asyncio.sleep(0.05)
            self.fail("polling did not converge")

        try:
            await until(lambda rows: len(rows) == 1)
            note.write_text("After")
            await until(lambda rows: rows and rows[0][2] == "After")
            note.unlink()
            await until(lambda rows: not rows)
        finally:
            process.send_signal(signal.SIGINT)
            try:
                await asyncio.to_thread(process.communicate, timeout=35)
            except subprocess.TimeoutExpired:
                process.kill()
                await asyncio.to_thread(process.communicate)
                self.fail("connector did not stop after the delivery timeout")
        self.assertEqual(process.returncode, 0)
