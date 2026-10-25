"""Token from the environment or `gh auth token`; `--check` never prints it."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_github import TOKEN

from mash_github.cli import main
from mash_github.client import TokenMissing, token


def gh(stdout: str = "", fails: bool = False):
    def run(
        arguments: list[str], **_: object
    ) -> subprocess.CompletedProcess[str] | subprocess.CompletedProcess[bytes]:
        if arguments[0] == "gh":
            if fails:
                raise FileNotFoundError("gh")
            return subprocess.CompletedProcess(arguments, 0, stdout, "")
        return subprocess.CompletedProcess(arguments, 0, b'{"silver_schema":54}', b"")

    return patch("subprocess.run", side_effect=run)


class TokenTests(unittest.TestCase):
    def test_environment_wins_then_gh_then_missing(self) -> None:
        with patch.dict(os.environ, {"GH_TEST_TOKEN": TOKEN}), gh(fails=True):
            self.assertEqual(token("GH_TEST_TOKEN"), TOKEN)
        with patch.dict(os.environ, {}, clear=True), gh(stdout=f"{TOKEN}\n"):
            self.assertEqual(token("GH_TEST_TOKEN"), TOKEN)
        with patch.dict(os.environ, {}, clear=True), gh(fails=True):
            with self.assertRaises(TokenMissing) as raised:
                token("GH_TEST_TOKEN")
        self.assertIn("GH_TEST_TOKEN", str(raised.exception))

    def test_check_reports_token_presence_without_printing_it(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "github.toml"
            config.write_text(
                "account_id='test'\nstate_dir='state'\nrepositories=['acme/app']\n"
                "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
            )
            arguments = ["--config", str(config), "--check"]
            with (
                patch.dict(os.environ, {}, clear=True),
                gh(stdout=TOKEN),
                patch("shutil.which", return_value="/mock/mash"),
            ):
                ready = CliRunner().invoke(main, arguments)
            with patch.dict(os.environ, {}, clear=True), gh(fails=True):
                missing = CliRunner().invoke(main, arguments)
            self.assertEqual(sorted(Path(directory).iterdir()), [config])
        self.assertEqual(ready.exit_code, 0)
        self.assertNotIn(TOKEN, ready.output)
        report = orjson.loads(ready.stdout)
        self.assertEqual(
            [(item["requirement"], item["status"]) for item in report["checks"]],
            [
                ("python", "passed"),
                ("token", "passed"),
                ("github_access", "unverified"),
                ("state", "passed"),
                ("mash", "passed"),
                ("database", "passed"),
            ],
        )
        self.assertEqual(missing.exit_code, 2)


if __name__ == "__main__":
    unittest.main()
