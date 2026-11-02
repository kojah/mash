"""Git history mapping over a throwaway repository."""

import subprocess
import tempfile
import unittest
from dataclasses import replace
from datetime import UTC, datetime
from pathlib import Path

from connectors.git_history import KIND_CHAT, KIND_COMMIT, Settings, read


def git(root: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)


class GitHistoryTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        git(self.root, "init", "-q")
        git(self.root, "config", "user.name", "Ada")
        git(self.root, "config", "user.email", "ada@example.com")
        (self.root / "a.txt").write_text("one\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-q", "-m", "first commit")
        (self.root / "a.txt").write_text("two\n")
        git(self.root, "add", ".")
        git(self.root, "commit", "-q", "-m", "second commit")

    def settings(self, **overrides: object) -> Settings:
        values = {"path": self.root, "account": "example.com/acme/app", "since_days": 3650}
        values.update(overrides)
        return Settings(**values)  # type: ignore[arg-type]

    def test_container_and_commits_are_emitted_newest_first(self) -> None:
        objects = read(self.settings())
        self.assertEqual(objects[0].kind, KIND_CHAT)
        self.assertEqual(objects[0].id, "example.com/acme/app")
        commits = [o for o in objects if o.kind == KIND_COMMIT]
        self.assertEqual(len(commits), 2)
        self.assertEqual(commits[0].content, "second commit")
        self.assertEqual(commits[1].content, "first commit")

    def test_commits_carry_author_parents_and_are_entries(self) -> None:
        commits = [o for o in read(self.settings()) if o.kind == KIND_COMMIT]
        newest = commits[0]
        self.assertEqual(newest.author.id, "ada@example.com")
        self.assertEqual(newest.author.name, "Ada")
        self.assertEqual(newest.author.kind, "human")
        self.assertEqual(newest.parent, "example.com/acme/app")
        self.assertEqual(len(newest.metadata["parents"]), 1)

    def test_the_window_bounds_history(self) -> None:
        future = datetime(2035, 1, 1, tzinfo=UTC)
        objects = read(self.settings(since_days=1), now=future)
        self.assertEqual([o.kind for o in objects], [KIND_CHAT])

    def test_source_key_is_repo_scoped_not_machine_scoped(self) -> None:
        self.assertEqual(self.settings().source_id, "git:example.com/acme/app")
        self.assertNotEqual(
            self.settings().source_id, self.settings(account="example.com/acme/other").source_id
        )


if __name__ == "__main__":
    unittest.main()
