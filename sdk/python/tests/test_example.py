"""The README's runnable example works end to end against a real Mash binary."""

import importlib.util
import os
import sqlite3
import tempfile
import unittest
from pathlib import Path

from mash_sdk import Destination

EXAMPLE = Path(__file__).parent.parent / "examples" / "markdown_folder.py"


def load_example():
    spec = importlib.util.spec_from_file_location("markdown_folder", EXAMPLE)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ExampleTests(unittest.IsolatedAsyncioTestCase):
    async def test_markdown_folder_polls_edits_and_deletions(self) -> None:
        binary = os.environ.get("MASH_CONNECTOR_MASH_BINARY")
        if binary is None:
            self.skipTest("set MASH_CONNECTOR_MASH_BINARY for actual CLI delivery")
        example = load_example()
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        folder = root / "notes"
        folder.mkdir()
        (folder / "cache.md").write_text("Warm the zeppelin cache first.")
        (folder / "deploy.md").write_text("Deploy after review.")
        config = root / "config.toml"
        config.write_text("version=1\n[providers]\n")
        database = root / "index.sqlite3"
        destination = Destination.local(
            database=database, executable=binary, config=config
        )
        state = root / "state.sqlite"

        def titles() -> set[str]:
            with sqlite3.connect(database) as connection:
                return {
                    row[0]
                    for row in connection.execute(
                        "SELECT title FROM objects WHERE origin='connector'"
                        " AND kind='markdown:note'"
                    )
                }

        await example.main(folder, destination, state, once=True)
        self.assertEqual(titles(), {"cache", "deploy"})
        (folder / "deploy.md").unlink()
        await example.main(folder, destination, state, once=True)
        self.assertEqual(titles(), {"cache"})


if __name__ == "__main__":
    unittest.main()
