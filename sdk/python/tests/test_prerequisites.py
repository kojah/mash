"""Destination checks are offline: presence never passes as verified authority."""

import os
import sqlite3
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from pydantic import ValidationError

from mash_sdk import Destination
from mash_sdk.prerequisites import Manifest, Status, check


def requirement(id: str, when: str = "always") -> dict[str, str]:
    return {"id": id, "when": when, "description": id, "remedy": f"fix {id}"}


MANIFEST = Manifest.model_validate(
    {
        "version": 1,
        "connector": "test",
        "python": [3, 11],
        "requirements": [
            requirement("python"),
            requirement("source"),
            requirement("mash", "local"),
            requirement("database", "local"),
            requirement("credentials", "shared"),
            requirement("authority", "shared"),
        ],
    }
)
SHARED = Destination.shared(
    api_url="https://mash.example",
    group_id="personal",
    node_id="laptop",
    token_env="MASH_PREREQUISITE_TEST_TOKEN",
)


class PrerequisiteTests(unittest.TestCase):
    def test_duplicate_requirements_are_rejected(self) -> None:
        with self.assertRaises(ValidationError):
            Manifest.model_validate(
                MANIFEST.model_dump() | {"requirements": [requirement("a")] * 2}
            )

    def test_shared_token_presence_is_ready_but_unverified(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            report = check(MANIFEST, {"source": Status.passed}, [SHARED])
        self.assertFalse(report.ready)
        with patch.dict(os.environ, {"MASH_PREREQUISITE_TEST_TOKEN": "secret"}):
            report = check(MANIFEST, {"source": Status.passed}, [SHARED])
        self.assertTrue(report.ready)
        self.assertFalse(report.fully_verified)
        self.assertEqual(
            [(item.requirement, item.status) for item in report.checks],
            [
                ("python", "passed"),
                ("source", "passed"),
                ("credentials", "passed"),
                ("authority", "unverified"),
            ],
        )
        self.assertEqual(report.checks[-1].remedy, "fix authority")
        self.assertNotIn("secret", report.model_dump_json())

    def test_local_schema_mismatch_fails_without_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "silver.sqlite3"
            with sqlite3.connect(database) as db:
                db.execute("PRAGMA user_version=53")
            local = Destination.local(database=database, executable="mash")
            with (
                patch("shutil.which", return_value="/mock/mash"),
                patch(
                    "subprocess.run",
                    return_value=subprocess.CompletedProcess(
                        ["mash"], 0, b'{"silver_schema":54}', b""
                    ),
                ),
            ):
                report = check(MANIFEST, {}, [local])
            statuses = {item.requirement: item.status for item in report.checks}
            self.assertEqual(statuses["mash"], "passed")
            self.assertEqual(statuses["database"], "failed")
            with sqlite3.connect(database) as db:
                self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], 53)


if __name__ == "__main__":
    unittest.main()
