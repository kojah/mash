"""Fake Drive records delivered into a disposable Mash database and found by search."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import orjson
from fake_google import ACCESS_TOKEN, REFRESH_TOKEN, FakeGoogle, item
from mash_sdk.config import Local

from mash_google_docs.config import Settings
from mash_google_docs.connector import Result, run


class IntegrationTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        binary = os.environ.get("MASH_CONNECTOR_MASH_BINARY")
        if binary is None:
            self.skipTest("set MASH_CONNECTOR_MASH_BINARY for real Mash delivery")
        self.binary = str(Path(binary).resolve())
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.database = self.root / "silver.sqlite3"
        self.config = self.root / "mash.toml"
        self.config.write_text(
            "version=1\n[providers]\n[views.build]\n"
            "representations='lexical-only'\n[views.query]\nintent='exact-mention'\n"
        )

    def mash(self, *arguments: str) -> dict:
        output = subprocess.run(
            [
                self.binary,
                "--config",
                str(self.config),
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

    def search(self, text: str) -> list[dict]:
        return self.mash(
            "search",
            text,
            "--local",
            "--source",
            "google-docs:test",
            "--intent",
            "exact-mention",
            "--unit",
            "messages",
        )["results"]

    async def test_doc_and_comment_are_searchable_and_viewable(self) -> None:
        google = FakeGoogle()
        google.add(
            item("doc1", parent="top"),
            text="# Plan\nThe zephyrlighthouse ships in May.",
            comments=[
                {
                    "id": "c1",
                    "author": {"displayName": "Lin", "permissionId": "7"},
                    "content": "Is the quokkaharbor ready?",
                    "quotedFileContent": {"value": "ships in May"},
                    "replies": [{"id": "r1", "content": "Yes, tested."}],
                }
            ],
        )
        settings = Settings(
            account_id="test",
            client_secret=self.root / "client.json",
            state_dir=self.root / "state",
            folders=("top",),
            destinations=(
                Local(
                    type="local",
                    database=self.database,
                    executable=self.binary,
                    config=self.config,
                ),
            ),
        )
        reports: list[Result] = []
        await run(settings, google.drive(), report=reports.append)
        self.assertEqual(reports[-1].exported_documents, 1)
        self.mash("index")
        found = self.search("zephyrlighthouse")
        self.assertTrue(found)
        viewed = self.mash("view", found[0]["lookup"]["view"]["id"])
        self.assertIn("zephyrlighthouse", orjson.dumps(viewed).decode())
        found = self.search("quokkaharbor")
        self.assertTrue(found)
        viewed = orjson.dumps(
            self.mash("view", found[0]["lookup"]["view"]["id"])
        ).decode()
        self.assertIn("Is the quokkaharbor ready?", viewed)
        self.assertTrue(self.search("tested"))
        # A second poll exports nothing and delivers only the schema declaration.
        await run(settings, google.drive(), report=reports.append)
        self.assertEqual(reports[-1].exported_documents, 0)
        for path in (self.root / "state").iterdir():
            for secret in (REFRESH_TOKEN, ACCESS_TOKEN):
                self.assertNotIn(secret.encode(), path.read_bytes())


if __name__ == "__main__":
    unittest.main()
