"""The token comes from the environment; `--check` and failures never print it."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_slack import TOKEN

from mash_slack.cli import main
from mash_slack.client import TokenMissing, token

CONFIG = (
    "team_id='T0TEAM'\nprincipal='U0ALICE'\nstate_dir='state'\nchannels=['C01']\n"
    "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
)


def mash_version(arguments: list[str], **_: object) -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(arguments, 0, b'{"silver_schema":54}', b"")


class TokenTests(unittest.TestCase):
    def test_token_comes_from_the_named_variable(self) -> None:
        with patch.dict(os.environ, {"SLACK_TEST_TOKEN": f" {TOKEN}\n"}):
            self.assertEqual(token("SLACK_TEST_TOKEN"), TOKEN)
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(TokenMissing) as raised:
                token("SLACK_TEST_TOKEN")
        self.assertIn("SLACK_TEST_TOKEN", str(raised.exception))

    def test_check_reports_token_presence_without_printing_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "slack.toml"
            config.write_text(CONFIG)
            arguments = ["--config", str(config), "--check"]
            with (
                patch.dict(os.environ, {"SLACK_TOKEN": TOKEN}, clear=True),
                patch("subprocess.run", side_effect=mash_version),
                patch("shutil.which", return_value="/mock/mash"),
            ):
                ready = CliRunner().invoke(main, arguments)
            with patch.dict(os.environ, {}, clear=True):
                missing = CliRunner().invoke(main, arguments)
                sync = CliRunner().invoke(main, ["--config", str(config), "sync"])
            self.assertEqual(sorted(Path(directory).iterdir()), [config])
        self.assertEqual(ready.exit_code, 0)
        self.assertNotIn(TOKEN, ready.output)
        report = orjson.loads(ready.stdout)
        self.assertEqual(
            [(item["requirement"], item["status"]) for item in report["checks"]],
            [
                ("python", "passed"),
                ("token", "passed"),
                ("slack_access", "unverified"),
                ("state", "passed"),
                ("mash", "passed"),
                ("database", "passed"),
            ],
        )
        self.assertEqual(missing.exit_code, 2)
        self.assertEqual(sync.exit_code, 3)


if __name__ == "__main__":
    unittest.main()
