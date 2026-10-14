"""Setup reports must distinguish missing access from unverified authority."""

import os
import sqlite3
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from mash_obsidian.config import Local, Settings, Shared
from mash_obsidian.prerequisites import Report, check, manifest


class PrerequisiteTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.vault = self.root / "vault"
        self.vault.mkdir()
        self.settings = Settings(
            vault=self.vault,
            vault_id="test",
            state_dir=self.root / "state",
            folders=(".",),
            destinations=(
                Local(
                    type="local",
                    database=self.root / "silver.sqlite3",
                ),
            ),
        )

    def local(self, settings: Settings | None = None) -> Report:
        with (
            patch("shutil.which", return_value="/mock/mash"),
            patch(
                "subprocess.run",
                return_value=subprocess.CompletedProcess(
                    ["mash", "--version"],
                    0,
                    b'{"silver_schema":54}',
                    b"",
                ),
            ),
        ):
            return check(settings or self.settings)

    def test_local_preflight_does_not_create_state_database_or_vault_files(
        self,
    ) -> None:
        before = sorted(self.root.rglob("*"))
        result = self.local()
        self.assertTrue(result.ready)
        self.assertTrue(result.fully_verified)
        self.assertEqual(sorted(self.root.rglob("*")), before)
        declared = manifest()
        self.assertEqual(
            len({r.id for r in declared.requirements}), len(declared.requirements)
        )

    def test_missing_folder_executable_and_incompatible_database_are_failures(
        self,
    ) -> None:
        with sqlite3.connect(self.root / "silver.sqlite3") as db:
            db.execute("PRAGMA user_version=53")
        settings = self.settings.model_copy(update={"folders": ("missing",)})
        result = self.local(settings)
        self.assertFalse(result.ready)
        checks = {item.requirement: item for item in result.checks}
        self.assertEqual(checks["vault"].status, "failed")
        self.assertEqual(checks["database"].status, "failed")
        with patch("shutil.which", return_value=None):
            self.assertFalse(check(self.settings).ready)
        with sqlite3.connect(self.root / "silver.sqlite3") as db:
            self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], 53)

    def test_shared_token_is_never_printed_or_mistaken_for_verified_authority(
        self,
    ) -> None:
        settings = self.settings.model_copy(
            update={
                "destinations": (
                    Shared(
                        type="shared",
                        api_url="https://example.invalid",
                        group_id="personal",
                        node_id="test",
                        token_env="OBSIDIAN_TEST_TOKEN",
                    ),
                )
            }
        )
        with patch.dict(os.environ, {}, clear=True), patch("subprocess.run") as process:
            result = check(settings)
            self.assertFalse(result.ready)
            process.assert_not_called()
        with patch.dict(os.environ, {"OBSIDIAN_TEST_TOKEN": "do-not-expose"}):
            result = check(settings)
            self.assertTrue(result.ready)
            self.assertFalse(result.fully_verified)
            self.assertNotIn("do-not-expose", repr(result))
            self.assertEqual(result.checks[-1].status, "unverified")
