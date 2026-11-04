"""Fake Slack messages delivered into a disposable Mash database and found by search."""

import os
import sqlite3
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_slack import TEAM, TOKEN, FakeSlack, limited, post, ts

from mash_slack.cli import main


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

    def source_id(self, prefix: str) -> str:
        """The one connector source this test delivered; keys may append a
        principal or node to the configured account."""
        with sqlite3.connect(self.database) as connection:
            [(source,)] = connection.execute(
                "SELECT DISTINCT source FROM objects WHERE origin='connector'"
            ).fetchall()
        self.assertTrue(source.startswith(prefix + ":") or source == prefix, source)
        return source

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
            self.source_id(f"slack:{TEAM}"),
            "--intent",
            "exact-mention",
            "--unit",
            "messages",
        )["results"]
        self.assertTrue(found, text)
        return orjson.dumps(
            self.mash("view", found[0]["lookup"]["view"]["id"])
        ).decode()

    def test_messages_and_thread_replies_are_searchable_and_viewable(self) -> None:
        now = int(time.time())
        parent = ts(now - 3600)
        with FakeSlack() as slack:
            slack.channel("C01", "general")
            slack.user("U1", "Ada")
            slack.user("U2", "Grace")
            slack.messages["C01"] = [
                post(parent, "The zephyrlighthouse deploy failed."),
                post(
                    ts(now - 60),
                    "Fixed the quokkaharbor guard.",
                    "U2",
                    thread_ts=parent,
                ),
                post(ts(now - 30), "joined", subtype="channel_join"),
            ]
            slack.failures["auth.test"] = [limited()]
            config = self.root / "slack.toml"
            (self.root / "slack.token").write_text(TOKEN)
            config.write_text(
                f"team_id='{TEAM}'\nprincipal='U0ALICE'\ntoken_file='slack.token'\n"
                "state_dir='state'\nchannels=['C01']\n"
                "[[destinations]]\ntype='local'\ndatabase='silver.sqlite3'\n"
                f"executable='{self.binary}'\nconfig='mash.toml'\n"
            )
            with patch("mash_slack.cli.connect", lambda _: slack.client()):
                first = CliRunner().invoke(main, ["--config", str(config), "sync"])
                second = CliRunner().invoke(main, ["--config", str(config), "sync"])
        self.assertEqual(first.exit_code, 0, first.output)
        self.assertEqual(orjson.loads(first.stdout)["synced_channels"], 1)
        # Nothing changed on Slack, so nothing is delivered again.
        self.assertEqual(orjson.loads(second.stdout)["delivered_batches"], 0)
        self.mash("index")
        self.assertIn("zephyrlighthouse deploy failed", self.view("zephyrlighthouse"))
        reply = self.view("quokkaharbor")
        self.assertIn("Fixed the quokkaharbor guard.", reply)
        for output in (first.output, second.output):
            self.assertNotIn(TOKEN, output)
        for path in (self.root / "state").iterdir():
            self.assertNotIn(TOKEN.encode(), path.read_bytes())


if __name__ == "__main__":
    unittest.main()
