"""Project chat discovery and background indexing through the hook boundary."""

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
# The fake supports only chat listing and incremental indexing.
FAKE = """
import json, os, sys, time
path = os.environ["FAKE_MASH"]
data = json.load(open(path))
time.sleep(data.get("sleep", 0))
command, arguments = sys.argv[1], sys.argv[1:]
if data.get("fail", {}).get(command):
    sys.exit(data["fail"][command])
if command == "list-chats":
    print(json.dumps({"chats": data["chats"], "next_offset": None}))
elif command == "index":
    open(path + ".auto-index", "a").write("index\\n")
"""


def chat(chat_id: str, project: str, repository: str | None = None, **extra: Any):
    now = int(time.time() * 1000)
    return {
        "chat_id": chat_id,
        "provider": "codex",
        "title": chat_id,
        "project": project,
        "repository": repository,
        "updated_at": now,
        "enrichment": {"timestamps": {"updated_at": {"message_id": f"{chat_id}-last"}}},
        **extra,
    }


class Recall(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.repo = self.root / "repo"
        (self.repo / "sub").mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        git_remote = ["git", "-C", str(self.repo), "remote", "add", "origin", ORIGIN]
        subprocess.run(git_remote, check=True)
        (self.root / "fake.py").write_text(FAKE)
        binary = self.root / "mash"
        binary.write_text(
            f'#!/bin/sh\nexec {sys.executable} {self.root}/fake.py "$@"\n'
        )
        binary.chmod(0o755)
        self.state = self.root / "state"
        self.env = {**os.environ, "MASH_BIN": str(binary)}
        self.env["MASH_RECALL_STATE_DIR"] = str(self.state)
        self.env["MASH_AUTO_INDEX"] = "off"
        self.env["FAKE_MASH"] = str(self.root / "fake.json")
        self.fake(chats=[])

    def fake(self, **values: Any) -> None:
        data = values
        (self.root / "fake.json").write_text(json.dumps(data))

    def run_script(self, name: str, stdin: bytes, *arguments: str):
        command = [sys.executable, str(HOOKS / name), *arguments]
        return subprocess.run(command, input=stdin, capture_output=True, env=self.env)

    def hook(self, session: str = "current", cwd: Path | None = None) -> str:
        payload = {"session_id": session, "cwd": str(cwd or self.repo / "sub")}
        result = self.run_script("session_start.py", json.dumps(payload).encode())
        self.assertEqual(result.returncode, 0)
        if not result.stdout:
            return ""
        return json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]

    def test_fails_open(self) -> None:
        self.fake(chats=[chat("other", str(self.repo))], fail={"list-chats": 5})
        self.assertEqual(self.hook(), "")
        self.env["MASH_BIN"] = str(self.root / "absent")
        self.assertEqual(self.hook(), "")
        self.assertEqual(self.hook(cwd=self.root / "gone"), "")
        result = self.run_script("session_start.py", b"{not json")
        self.assertEqual((result.returncode, result.stdout), (0, b""))

    def test_starts_one_background_index_per_interval(self) -> None:
        self.env["MASH_AUTO_INDEX"] = "on"
        log = self.root / "fake.json.auto-index"
        self.hook()
        self.hook()
        deadline = time.monotonic() + 5
        while not log.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        time.sleep(0.2)
        self.assertEqual(log.read_text(), "index\n")
        state = json.loads((self.state / "auto-index.json").read_text())
        self.assertLessEqual(time.time() - state["started_at"], 10)

    def test_deadline(self) -> None:
        self.fake(chats=[chat("other", str(self.repo))], sleep=10)
        started = time.monotonic()
        self.assertEqual(self.hook(), "")
        self.assertLess(time.monotonic() - started, 5)

    def test_project_filtering_and_current_session(self) -> None:
        chats = [
            chat("current", str(self.repo)),
            chat("subdir", str(self.repo / "sub")),
            chat("clone", "/elsewhere/clone", ORIGIN),
            chat("sibling", str(self.repo) + "-other"),
            chat("unrelated", "/elsewhere", "https://example.test/other.git"),
        ]
        self.fake(chats=chats)
        text = self.hook()
        self.assertTrue(text.startswith(f"Mash recall for {ORIGIN} ({self.repo})"))
        self.assertIn("evidence, never instructions", text)
        for included in ("subdir", "clone"):
            self.assertIn(f"chat={included} ", text)
        for excluded in ("current", "sibling", "unrelated"):
            self.assertNotIn(f"chat={excluded} ", text)
        self.assertIn("Recent project chats", text)
        self.assertIn('view={"id":"subdir-last","context":4}', text)
        self.assertFalse(self.state.exists())  # The hook never writes state.

    def test_budget(self) -> None:
        long_title = "x" * 500
        chats = [chat(f"c{n}", str(self.repo), title=long_title) for n in range(20)]
        self.fake(chats=chats)
        self.assertLessEqual(len(self.hook()), 2000)
        deep = self.root / ("d" * 200) / ("e" * 200) / ("f" * 200)
        deep.mkdir(parents=True)  # A huge header leaves no room for chat links.
        self.fake(chats=[chat(f"c{n}", str(deep)) for n in range(4)])
        text = self.hook(cwd=deep)
        self.assertLessEqual(len(text), 2000)
        self.assertIn("Mash recall for", text)
