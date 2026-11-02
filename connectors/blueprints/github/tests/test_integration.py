"""Fake GitHub records delivered into a disposable Mash database and found by search."""

import os
import sqlite3
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_github import TOKEN, FakeGitHub, comment, issue, review, review_comment

from mash_github.cli import main


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
            self.source_id("github:test"),
            "--intent",
            "exact-mention",
            "--unit",
            "messages",
        )["results"]
        self.assertTrue(found, text)
        return orjson.dumps(
            self.mash("view", found[0]["lookup"]["view"]["id"])
        ).decode()

    def test_issue_and_review_comment_are_searchable_and_viewable(self) -> None:
        github = FakeGitHub()
        github.add(
            "acme/app",
            issue("acme/app", 1, body="The zephyrlighthouse crashes on start."),
            issue("acme/app", 2, pull=True, body="Fix the crash."),
        )
        github.comments["acme/app#1"] = [comment(11, "Reproduced on Linux.")]
        github.reviews["acme/app#2"] = [review(21, "Looks good.")]
        github.review_comments["acme/app#2"] = [
            review_comment(31, "Is the quokkaharbor guard needed?"),
            review_comment(32, "Yes, for empty input.", reply_to=31),
        ]
        config = self.root / "github.toml"
        config.write_text(
            "account_id='test'\nstate_dir='state'\nrepositories=['acme/app']\n"
            "[[destinations]]\ntype='local'\ndatabase='silver.sqlite3'\n"
            f"executable='{self.binary}'\nconfig='mash.toml'\n"
        )
        with (
            patch.dict(os.environ, {"GITHUB_TOKEN": TOKEN}),
            patch("mash_github.cli.connect", lambda _: github.github()),
        ):
            first = CliRunner().invoke(main, ["--config", str(config), "sync"])
            second = CliRunner().invoke(main, ["--config", str(config), "sync"])
        self.assertEqual(first.exit_code, 0, first.output)
        self.assertEqual(orjson.loads(first.stdout)["synced_items"], 2)
        # Nothing changed on GitHub, so the second poll lists nothing new.
        self.assertEqual(orjson.loads(second.stdout)["listed_items"], 0)
        self.mash("index")
        self.assertIn("zephyrlighthouse crashes", self.view("zephyrlighthouse"))
        self.assertIn("Reproduced on Linux.", self.view("Reproduced"))
        self.assertIn("quokkaharbor guard needed?", self.view("quokkaharbor"))
        self.assertIn("Yes, for empty input.", self.view("empty input"))
        for output in (first.output, second.output):
            self.assertNotIn(TOKEN, output)
        for path in (self.root / "state").iterdir():
            self.assertNotIn(TOKEN.encode(), path.read_bytes())


if __name__ == "__main__":
    unittest.main()
