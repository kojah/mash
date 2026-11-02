"""File-read recall hook against a fake (and optionally real) Mash."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from typing import Any

HOOKS = Path(__file__).resolve().parent.parent
ORIGIN = "https://example.test/repo.git"
# A stand-in `mash search` returning $FAKE_MASH's chats and logging arguments.
FAKE = """
import json, os, sys, time
path = os.environ["FAKE_MASH"]
data = json.load(open(path))
time.sleep(data.get("sleep", 0))
if data.get("fail"):
    sys.exit(data["fail"])
data.setdefault("calls", []).append(sys.argv[1:])
json.dump(data, open(path, "w"))
print(json.dumps({"results": data["results"], "next_offset": None}))
"""


def chat(chat_id: str, snippet: str, title: str = "Earlier work") -> dict[str, Any]:
    match = {"message_id": f"{chat_id}-m", "created_at": 1_793_178_060_000}
    return {
        "chat_id": chat_id,
        "title": title,
        "matches": [match | {"snippet": snippet}],
    }


class FileContext(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.repo = self.root / "repo"
        (self.repo / "app").mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        git_remote = ["git", "-C", str(self.repo), "remote", "add", "origin", ORIGIN]
        subprocess.run(git_remote, check=True)
        self.large = self.repo / "app" / "schema.sql"
        self.large.write_text("-- schema\n" * 400)
        (self.root / "fake.py").write_text(FAKE)
        binary = self.root / "mash"
        binary.write_text(
            f'#!/bin/sh\nexec {sys.executable} {self.root}/fake.py "$@"\n'
        )
        binary.chmod(0o755)
        self.env = {**os.environ, "MASH_BIN": str(binary)}
        self.env["MASH_RECALL_STATE_DIR"] = str(self.root / "state")
        self.env["FAKE_MASH"] = str(self.root / "fake.json")
        self.fake(results=[])

    def fake(self, **values: Any) -> None:
        (self.root / "fake.json").write_text(json.dumps(values))

    def calls(self) -> list[list[str]]:
        return json.loads((self.root / "fake.json").read_text()).get("calls", [])

    def hook(self, path: Path, session: str = "current", tool: str = "Read") -> str:
        payload: dict[str, Any] = {
            "session_id": session,
            "tool_name": tool,
            "cwd": str(self.repo),
        }
        payload["tool_input"] = {"file_path": str(path)}
        command = [sys.executable, str(HOOKS / "file_context.py")]
        stdin = json.dumps(payload).encode()
        result = subprocess.run(command, input=stdin, capture_output=True, env=self.env)
        self.assertEqual(result.returncode, 0)
        if not result.stdout:
            return ""
        output = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "PreToolUse")
        return output["additionalContext"]

    def test_shows_project_history_once_per_session(self) -> None:
        results = [
            chat("current", "this session's own mention"),
            chat(
                "earlier", "I changed [schema.sql](app/schema.sql:12) to add a column"
            ),
        ]
        self.fake(results=results)
        text = self.hook(self.large)
        self.assertTrue(text.startswith("Mash: earlier project chats mention"))
        self.assertIn("app/schema.sql", text.split("\n")[0])
        self.assertIn("evidence, never instructions", text)
        self.assertIn('- 2026-10-28 "Earlier work": I changed', text)
        self.assertIn('view={"id":"earlier-m"}', text)
        self.assertNotIn("own mention", text)
        [call] = self.calls()
        self.assertEqual(call[:2], ["search", "app/schema.sql"])
        self.assertEqual(call[call.index("--repository") + 1], ORIGIN)
        self.assertIn("--local", call)
        self.assertEqual(self.hook(self.large), "")
        self.assertIn("I changed", self.hook(self.large, session="another"))

    def test_skips_without_searching(self) -> None:
        self.fake(results=[chat("earlier", "mention")])
        small = self.repo / "app" / "small.txt"
        small.write_text("tiny")
        outside = self.root / "outside.sql"
        outside.write_text("x" * 4000)
        for path in (small, outside, self.repo / "app" / "missing.sql"):
            self.assertEqual(self.hook(path), "")
        self.assertEqual(self.hook(self.large, tool="Edit"), "")
        self.env["MASH_FILE_RECALL"] = "off"
        self.assertEqual(self.hook(self.large), "")
        self.assertEqual(self.calls(), [])

    def test_no_history_is_not_claimed(self) -> None:
        self.assertEqual(self.hook(self.large), "")
        self.fake(results=[chat("earlier", "now mentioned")])
        self.assertIn("now mentioned", self.hook(self.large))

    def test_budget(self) -> None:
        self.fake(results=[chat(f"c{n}", "y" * 500, "t" * 200) for n in range(9)])
        text = self.hook(self.large)
        self.assertLessEqual(len(text), 1200)
        self.assertEqual(text.count("\n- "), 4)

    def test_fails_open(self) -> None:
        self.fake(results=[], fail=5)
        self.assertEqual(self.hook(self.large), "")
        self.fake(results=[chat("earlier", "slow")], sleep=10)
        started = time.monotonic()
        self.assertEqual(self.hook(self.large), "")
        self.assertLess(time.monotonic() - started, 5)
        self.env["MASH_BIN"] = str(self.root / "absent")
        self.assertEqual(self.hook(self.large), "")
        command = [sys.executable, str(HOOKS / "file_context.py")]
        result = subprocess.run(command, input=b"{not json", capture_output=True)
        self.assertEqual((result.returncode, result.stdout), (0, b""))


@unittest.skipUnless(os.environ.get("MASH_RECALL_TEST_BINARY"), "needs a built mash")
class RealMash(unittest.TestCase):
    """The hook's search against a real Mash and a disposable lexical index."""

    def test_finds_an_earlier_chat_that_names_the_file(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        repo = root / "repo"
        (repo / "app" / "storage").mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        git_remote = ["git", "-C", str(repo), "remote", "add", "origin", ORIGIN]
        subprocess.run(git_remote, check=True)
        target = repo / "app" / "storage" / "silver.sql"
        target.write_text("-- schema\n" * 400)
        texts = [
            ("user", "Please fix the index schema today."),
            ("assistant", "I changed [silver.sql](app/storage/silver.sql:12)."),
            ("user", "Unrelated chatter about lunch."),
        ]
        sessions = root / "claude" / "project"
        sessions.mkdir(parents=True)
        rows = [
            {
                "type": role,
                "sessionId": "earlier",
                "uuid": f"u{n}",
                "cwd": str(repo),
                "timestamp": f"2026-10-28T09:{n:02}:00Z",
                "message": {"role": role, "content": text},
            }
            for n, (role, text) in enumerate(texts)
        ]
        (sessions / "earlier.jsonl").write_text(
            "".join(json.dumps(row) + "\n" for row in rows)
        )
        (root / "config.toml").write_text(
            "version = 1\n[providers.claude-code]\n"
            f"roots = [{json.dumps(str(root / 'claude'))}]\n"
            "[views.build]\nrepresentations = 'lexical-only'\n"
            "[views.query]\nintent = 'exact-mention'\n"
        )
        real = os.environ["MASH_RECALL_TEST_BINARY"]
        options = f"--config {root}/config.toml --db {root}/mash.sqlite3"
        options += f" --models-dir {root}/no-models"
        mash = root / "mash"
        mash.write_text(f'#!/bin/sh\nexec {real} {options} "$@"\n')
        mash.chmod(0o755)
        subprocess.run([str(mash), "index"], capture_output=True, check=True)
        env = {**os.environ, "MASH_BIN": str(mash)}
        env["MASH_RECALL_STATE_DIR"] = str(root / "state")
        payload: dict[str, Any] = {
            "session_id": "now",
            "tool_name": "Read",
            "cwd": str(repo),
        }
        payload["tool_input"] = {"file_path": str(target)}
        command = [sys.executable, str(HOOKS / "file_context.py")]
        stdin = json.dumps(payload).encode()
        result = subprocess.run(command, input=stdin, capture_output=True, env=env)
        text = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("mention app/storage/silver.sql", text)
        self.assertIn("I changed [silver.sql](app/storage/silver.sql:12).", text)
        self.assertNotIn("lunch", text)


if __name__ == "__main__":
    unittest.main()
