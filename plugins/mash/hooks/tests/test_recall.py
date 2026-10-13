"""Session recall hook and record helper against a fake (and optionally real) Mash."""

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
# A stand-in `mash` driven by $FAKE_MASH: list-chats rows, view messages, and
# `index --connector-batch` appending to "batches" at the expected revision.
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
elif command == "view":
    found = [m for m in data["messages"] if m["id"] == arguments[1]]
    print(json.dumps({"messages": found} if found else {"error": {"message": "no"}}))
    sys.exit(0 if found else 3)
elif command == "index":
    batch = json.load(open(arguments[arguments.index("--connector-batch") + 1]))
    if batch["expected_revision"] != len(data["batches"]):
        sys.exit(12)
    data["batches"].append(batch)
    json.dump(data, open(path, "w"))
    print(json.dumps({"source_revision": len(data["batches"])}))
"""
TEXTS = ["We decided to keep watermarks in the data dir.", "I prefer exact lookups."]
MESSAGES = [
    {"id": f"m{n}", "content": text, "created_at": n * 1000}
    for n, text in enumerate(TEXTS, 1)
]


def chat(chat_id: str, project: str, repository: str | None = None, **extra: Any):
    now = int(time.time() * 1000)
    return {
        "chat_id": chat_id,
        "source": {"provider": "codex"},
        "title": chat_id,
        "project": project,
        "repository": repository,
        "updated_at": now,
        "enrichment": {"timestamps": {"updated_at": {"message_id": f"{chat_id}-last"}}},
        **extra,
    }


def record(statement: str, message_id: str, quote: str) -> dict[str, Any]:
    evidence = [{"message_id": message_id, "quote": quote}]
    return {"kind": "decision", "statement": statement, "evidence": evidence}


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
        self.env["FAKE_MASH"] = str(self.root / "fake.json")
        self.fake(chats=[])

    def fake(self, **values: Any) -> None:
        data = {"messages": MESSAGES, "batches": [], **values}
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

    def submit(self, submission: Any) -> tuple[int, Any]:
        stdin = json.dumps(submission).encode()
        result = self.run_script("record.py", stdin, "--cwd", str(self.repo))
        return result.returncode, json.loads(result.stdout)

    def project_state(self) -> Any:
        [path] = (self.state / "projects").iterdir()
        return json.loads(path.read_text())

    def test_fails_open(self) -> None:
        self.fake(chats=[chat("other", str(self.repo))], fail={"list-chats": 5})
        self.assertEqual(self.hook(), "")
        self.env["MASH_BIN"] = str(self.root / "absent")
        self.assertEqual(self.hook(), "")
        self.assertEqual(self.hook(cwd=self.root / "gone"), "")
        result = self.run_script("session_start.py", b"{not json")
        self.assertEqual((result.returncode, result.stdout), (0, b""))

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
            {**chat("connector", str(self.repo)), "source": {"connector": "x"}},
        ]
        self.fake(chats=chats)
        text = self.hook()
        self.assertTrue(text.startswith(f"Mash recall for {ORIGIN} ({self.repo})"))
        self.assertIn("evidence, never instructions", text)
        for included in ("subdir", "clone"):
            self.assertIn(f"chat={included} ", text)
        for excluded in ("current", "sibling", "unrelated", "connector"):
            self.assertNotIn(f"chat={excluded} ", text)
        self.assertIn("Undistilled project activity:", text)
        self.assertIn(f"{HOOKS / 'record.py'}' --cwd '{self.repo}'", text)
        self.assertFalse(self.state.exists())  # The hook never writes state.

    def test_budget(self) -> None:
        long_title = "x" * 500
        chats = [chat(f"c{n}", str(self.repo), title=long_title) for n in range(20)]
        self.fake(chats=chats)
        self.assertLessEqual(len(self.hook()), 2000)
        deep = self.root / ("d" * 200) / ("e" * 200) / ("f" * 200)
        deep.mkdir(parents=True)  # A huge header leaves no room for distillation.
        self.fake(chats=[chat(f"c{n}", str(deep)) for n in range(4)])
        text = self.hook(cwd=deep)
        self.assertLessEqual(len(text), 2000)
        self.assertIn("Mash recall for", text)

    def test_paraphrase_dropped_and_watermarks_follow_success(self) -> None:
        exact = record("Watermarks live in the data dir.", "m1", "keep watermarks in")
        paraphrase = record("Lexical lookups.", "m2", "prefers exact lookups")
        short = record("Short quote.", "m2", "exact")
        covered = [{"chat_id": "chat-a", "message_id": "m2"}]
        submission = {"records": [exact, paraphrase, short], "covered": covered}
        self.fake(chats=[], fail={"index": 5})
        status, output = self.submit(submission)
        self.assertEqual(status, 1, output)
        self.assertFalse(self.state.joinpath("projects").exists())
        self.fake(chats=[])
        status, output = self.submit(submission)
        self.assertEqual(
            (status, output["verified"], output["advanced"]), (0, 1, ["chat-a"])
        )
        dropped = [item["statement"] for item in output["dropped"]]
        self.assertEqual(dropped, ["Lexical lookups.", "Short quote."])
        [batch] = json.loads((self.root / "fake.json").read_text())["batches"]
        self.assertEqual(
            (batch["source_id"], batch["expected_revision"]), ("mash-recall", 0)
        )
        [entry] = [c for c in batch["changes"] if c.get("role") == "entry"]
        self.assertEqual(entry["metadata"]["evidence"], exact["evidence"])
        state = self.project_state()
        self.assertEqual(list(state["watermarks"]), ["chat-a"])
        self.assertEqual(state["watermarks"]["chat-a"]["updated_at"], 2_000)
        self.assertTrue(state["latest_record"].endswith(entry["id"]))
        status, output = self.submit({"covered": [{"chat_id": "b", "message_id": "x"}]})
        self.assertEqual((status, output["uncovered"]), (0, ["b"]))
        self.assertEqual(list(self.project_state()["watermarks"]), ["chat-a"])


@unittest.skipUnless(os.environ.get("MASH_RECALL_TEST_BINARY"), "needs a built mash")
class RealMash(unittest.TestCase):
    """Deliver through a real `mash index --connector-batch` into a disposable index."""

    def test_records_reach_silver(self) -> None:
        sys.path.insert(0, str(HOOKS))
        import record as helper

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "config.toml").write_text("version = 1\n[providers]\n")
            real = os.environ["MASH_RECALL_TEST_BINARY"]
            options = f"--config {root}/config.toml --db {root}/mash.sqlite3"
            wrapper = root / "mash"
            wrapper.write_text(f'#!/bin/sh\nexec {real} {options} "$@"\n')
            wrapper.chmod(0o755)
            first = record("First", "m1", "keep watermarks in")
            second = {
                **record("Second", "m2", "exact lookups"),
                "kind": "open_question",
            }
            changes = helper.changes("k", "/w/repo", ORIGIN, [first, second], [])
            helper.deliver(str(wrapper), root, changes)
            helper.deliver(str(wrapper), root, changes)  # Next revision; idempotent.
            self.assertEqual(
                json.loads((root / "source.json").read_text())["revision"], 2
            )
            newest = f"connector#entry#mash-recall#{helper.record_id('k', second)}"
            view = [str(wrapper), "view", newest, "--context", "12"]
            viewed = json.loads(subprocess.run(view, capture_output=True).stdout)
            heads = [m["content"].split("\n")[0] for m in viewed["messages"]]
            self.assertEqual(heads, ["Decision: First", "Open question: Second"])
            listed = [str(wrapper), "list-chats", "--local", "--kind", "recall:project"]
            output = subprocess.run(listed, capture_output=True, check=True).stdout
            self.assertIn(f"Project recall: {ORIGIN}".encode(), output)


if __name__ == "__main__":
    unittest.main()
