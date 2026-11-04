"""The token comes from the environment; `--check` and failures never print it."""

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
from mash_slack.config import load

CONFIG = (
    "team_id='T0TEAM'\nprincipal='U0ALICE'\ntoken_file='slack.token'\n"
    "state_dir='state'\nchannels=['C01']\n"
    "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
)


def mash_version(arguments: list[str], **_: object) -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(arguments, 0, b'{"silver_schema":54}', b"")


class ExampleTests(unittest.TestCase):
    def test_the_shipped_example_is_a_valid_configuration(self) -> None:
        settings = load(Path(__file__).parents[1] / "example.toml")
        self.assertEqual(settings.source_id, "slack:T0123456789:U0123456789")


class TokenTests(unittest.TestCase):
    def test_token_comes_from_the_saved_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            saved = Path(directory) / "slack.token"
            with self.assertRaises(TokenMissing) as raised:
                token(saved)
            self.assertIn("mash-slack login", str(raised.exception))
            saved.write_text(f" {TOKEN}\n")
            self.assertEqual(token(saved), TOKEN)

    def test_check_reports_token_presence_without_printing_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "slack.toml"
            config.write_text(CONFIG)
            saved = Path(directory) / "slack.token"
            arguments = ["--config", str(config), "--check"]
            saved.write_text(TOKEN)
            with (
                patch("subprocess.run", side_effect=mash_version),
                patch("shutil.which", return_value="/mock/mash"),
            ):
                ready = CliRunner().invoke(main, arguments)
            saved.unlink()
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
