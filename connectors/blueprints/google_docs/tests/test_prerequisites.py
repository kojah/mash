"""`--check` is offline: private secrets pass; nothing is created or printed."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import orjson
from click.testing import CliRunner
from fake_google import CLIENT_SECRET, REFRESH_TOKEN

from mash_google_docs.auth import SCOPE, StoredToken, save_token
from mash_google_docs.cli import main


class PrerequisiteTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.client = self.root / "client.json"
        self.client.write_bytes(
            orjson.dumps(
                {"installed": {"client_id": "id", "client_secret": CLIENT_SECRET}}
            )
        )
        self.client.chmod(0o600)
        self.config = self.root / "google-docs.toml"
        self.config.write_text(
            "account_id='test'\nclient_secret='client.json'\ntoken='token.json'\n"
            "state_dir='state'\nfolders=['top']\n"
            "[[destinations]]\ntype='local'\ndatabase='mash.sqlite3'\n"
        )

    def check(self) -> tuple[int, dict[str, str], str]:
        with (
            patch("shutil.which", return_value="/mock/mash"),
            patch(
                "subprocess.run",
                return_value=subprocess.CompletedProcess(
                    ["mash", "--version"], 0, b'{"silver_schema":54}', b""
                ),
            ),
        ):
            result = CliRunner().invoke(main, ["--config", str(self.config), "--check"])
        report = orjson.loads(result.stdout)
        statuses = {item["requirement"]: item["status"] for item in report["checks"]}
        return result.exit_code, statuses, result.output

    def test_missing_token_and_shared_client_secret_fail(self) -> None:
        os.chmod(self.client, 0o644)
        before = sorted(self.root.rglob("*"))
        code, statuses, output = self.check()
        self.assertEqual(code, 2)
        self.assertEqual(statuses["client_secret"], "failed")
        self.assertEqual(statuses["token"], "failed")
        self.assertEqual(statuses["state"], "passed")
        self.assertEqual(sorted(self.root.rglob("*")), before)
        self.assertNotIn(CLIENT_SECRET, output)

    def test_private_files_are_ready_but_authorization_stays_unverified(self) -> None:
        save_token(
            self.root / "token.json",
            StoredToken.model_validate(
                {"refresh_token": REFRESH_TOKEN, "scope": SCOPE}
            ),
        )
        code, statuses, output = self.check()
        self.assertEqual(code, 0)
        self.assertEqual(
            statuses,
            {
                "python": "passed",
                "client_secret": "passed",
                "token": "passed",
                "authorization": "unverified",
                "state": "passed",
                "mash": "passed",
                "database": "passed",
            },
        )
        self.assertFalse(orjson.loads(output)["fully_verified"])
        for secret in (CLIENT_SECRET, REFRESH_TOKEN):
            self.assertNotIn(secret, output)


if __name__ == "__main__":
    unittest.main()
