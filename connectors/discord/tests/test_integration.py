"""Fake Discord messages delivered into a disposable Mash database, then searched."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_discord import HELPER, TOKEN, FakeDiscord, channel, message, snowflake

from mash_discord import client
from mash_discord.cli import main

GENERAL = "200000000000000000"
THREAD = "300000000000000000"


class IntegrationTests(unittest.TestCase):
    def setUp(self) -> None:
        binary = os.environ.get("MASH_CONNECTOR_MASH_BINARY")
        if binary is None:
            self.skipTest("set MASH_CONNECTOR_MASH_BINARY for real Mash delivery")
        self.binary = str(Path(binary).resolve())
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.database = self.root / "silver.sqlite3"
        self.mash_config = self.root / "mash.toml"
        self.mash_config.write_text(
            "version=1\n[providers]\n[views.build]\n"
            "representations='lexical-only'\n[views.query]\nintent='exact-mention'\n"
        )

    def mash(self, *arguments: str) -> dict:
        output = subprocess.run(
            [
                self.binary,
                "--config",
                str(self.mash_config),
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

    def view(self, text: str) -> str:
        found = self.mash(
            "search",
            text,
            "--local",
            "--source",
            "discord:test",
            "--intent",
            "exact-mention",
            "--unit",
            "messages",
        )["results"]
        self.assertTrue(found, text)
        return orjson.dumps(
            self.mash("view", found[0]["lookup"]["view"]["id"])
        ).decode()

    def test_channel_and_thread_messages_are_searchable_and_viewable(self) -> None:
        discord = FakeDiscord()
        question = snowflake("2026-10-01T00:00:00+00:00")
        discord.add(
            channel(GENERAL, "general"),
            message(GENERAL, question, "Does the zephyrlighthouse build pass?"),
            message(
                GENERAL,
                snowflake("2026-10-01T00:01:00+00:00"),
                "It passes after the quokkaharbor fix.",
                author=HELPER,
                type=19,
                reply_to=question,
            ),
        )
        discord.add(
            channel(THREAD, "release plan", parent=GENERAL),
            message(THREAD, snowflake("2026-10-02T00:00:00+00:00"), "Tag on Friday."),
        )
        config = self.root / "discord.toml"
        config.write_text(
            f"account_id='test'\nstate_dir='state'\nchannels=['{GENERAL}','{THREAD}']\n"
            "[[destinations]]\ntype='local'\ndatabase='silver.sqlite3'\n"
            f"executable='{self.binary}'\nconfig='mash.toml'\n"
        )
        with discord.serve() as url:
            fake = lambda secret: client.connect(secret, url)  # noqa: E731
            with (
                patch.dict(os.environ, {"DISCORD_BOT_TOKEN": TOKEN}),
                patch("mash_discord.cli.connect", fake),
            ):
                first = CliRunner().invoke(main, ["--config", str(config), "sync"])
                second = CliRunner().invoke(main, ["--config", str(config), "sync"])
        self.assertEqual(first.exit_code, 0, first.output)
        self.assertEqual(orjson.loads(first.stdout)["synced_messages"], 3)
        # Nothing new on Discord, so the second poll fetches nothing.
        self.assertEqual(orjson.loads(second.stdout)["fetched_messages"], 0)
        self.mash("index")
        self.assertIn("zephyrlighthouse build pass?", self.view("zephyrlighthouse"))
        self.assertIn("quokkaharbor fix", self.view("quokkaharbor"))
        self.assertIn("Tag on Friday.", self.view("Friday"))
        for output in (first.output, second.output):
            self.assertNotIn(TOKEN, output)
        for path in (self.root / "state").iterdir():
            self.assertNotIn(TOKEN.encode(), path.read_bytes())


if __name__ == "__main__":
    unittest.main()
