"""The bot token comes from the environment; no output ever contains it."""

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_discord import TOKEN

from mash_discord.cli import main
from mash_discord.client import TokenMissing, token


class TokenTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.config = self.root / "discord.toml"
        self.config.write_text(
            "account_id='test'\ntoken_env='DISCORD_TEST_TOKEN'\nstate_dir='state'\n"
            "channels=['200000000000000000']\n"
            "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
        )

    def test_token_is_read_from_the_named_variable(self) -> None:
        with patch.dict(os.environ, {"DISCORD_TEST_TOKEN": f" {TOKEN}\n"}):
            self.assertEqual(token("DISCORD_TEST_TOKEN"), TOKEN)
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(TokenMissing) as raised:
                token("DISCORD_TEST_TOKEN")
        self.assertIn("DISCORD_TEST_TOKEN", str(raised.exception))

    def test_check_reports_token_presence_without_printing_it(self) -> None:
        arguments = ["--config", str(self.config), "--check"]
        with (
            patch.dict(os.environ, {"DISCORD_TEST_TOKEN": TOKEN}, clear=True),
            patch("shutil.which", return_value=None),
        ):
            report = CliRunner().invoke(main, arguments)
        with patch.dict(os.environ, {}, clear=True):
            missing = CliRunner().invoke(main, arguments)
        self.assertEqual(sorted(self.root.iterdir()), [self.config])
        self.assertNotIn(TOKEN, report.output)
        checks = orjson.loads(report.stdout)["checks"]
        self.assertEqual(
            [(item["requirement"], item["status"]) for item in checks],
            [
                ("python", "passed"),
                ("token", "passed"),
                ("discord_access", "unverified"),
                ("state", "passed"),
                ("mash", "failed"),  # No Mash executable on this PATH.
                ("database", "passed"),  # Created on first delivery.
            ],
        )
        self.assertEqual(missing.exit_code, 2)
        self.assertEqual(orjson.loads(missing.stdout)["checks"][1]["status"], "failed")

    def test_sync_without_a_token_exits_3(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            result = CliRunner().invoke(main, ["--config", str(self.config), "sync"])
        self.assertEqual(result.exit_code, 3)
        self.assertIn("token-missing", result.stderr)


if __name__ == "__main__":
    unittest.main()
