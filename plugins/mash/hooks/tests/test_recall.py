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
PREFIX = "connector#entry#mash-recall#"
# A stand-in `mash` driven by $FAKE_MASH: list-chats rows, view messages, and
# `index --connector-batch` appending to "batches" at the expected revision.
FAKE = """
import json, os, sys, time
path = os.environ["FAKE_MASH"]
PREFIX = "connector#entry#mash-recall#"
data = json.load(open(path))
time.sleep(data.get("sleep", 0))
command, arguments = sys.argv[1], sys.argv[1:]
if data.get("fail", {}).get(command):
    sys.exit(data["fail"][command])
if command == "list-chats":
    print(json.dumps({"chats": data["chats"], "next_offset": None}))
elif command == "view":
    # Like Mash: an unknown ID is an empty result; a record's context is the
    # other records of its project.
    found = [m for m in data["messages"] if m["id"] == arguments[1]]
    if found and found[0]["id"].startswith(PREFIX) and arguments[-1] != "0":
        found = [m for m in data["messages"] if m["id"].startswith(PREFIX)]
        found.sort(key=lambda m: m["created_at"])
    print(json.dumps({"messages": found}))
elif command == "index":
    batch = json.load(open(arguments[arguments.index("--connector-batch") + 1]))
    if batch["expected_revision"] != len(data["batches"]):
        sys.exit(12)
    data["batches"].append(batch)
    for change in batch["changes"]:
        if change.get("role") == "entry":
            entry = {"id": PREFIX + change["id"], "author_kind": "bot"}
            entry.update(content=change["content"], created_at=change["created_at"])
            rest = [m for m in data["messages"] if m["id"] != entry["id"]]
            data["messages"] = rest + [entry]
    json.dump(data, open(path, "w"))
    print(json.dumps({"source_revision": len(data["batches"])}))
"""
TEXTS = [
    ("user", "We decided to keep watermarks in the data dir."),
    ("user", "I prefer exact lookups."),
    ("user", "We will lint the harness with Biome across the monorepo."),
    ("assistant", "I suggest switching from Biome to oxlint for faster lint rules."),
    ("user", "Yes, go with oxlint and drop Biome from the toolchain."),
    ("user", "Use oxlint with type-aware rules through tsgolint as well."),
]
MESSAGES = [
    {"id": f"m{n}", "role": role, "content": text, "created_at": n * 1000}
    for n, (role, text) in enumerate(TEXTS, 1)
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


def record(
    statement: str, message_id: str, quote: str, *supersedes: dict[str, str]
) -> dict[str, Any]:
    evidence = [{"message_id": message_id, "quote": quote}]
    result = {"kind": "decision", "statement": statement, "evidence": evidence}
    return {**result, "supersedes": list(supersedes)} if supersedes else result


OXLINT_HEADING = "Decision: Lint with oxlint instead of Biome"
BIOME = record("Lint with Biome", "m3", "lint the harness with Biome")
OXLINT = record(
    "Lint with oxlint instead of Biome",
    "m5",
    "go with oxlint and drop Biome",
    {"statement": "Lint with Biome"},
)


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

    def test_records_rank_by_confirmation(self) -> None:
        exact = record("Watermarks live in the data dir.", "m1", "keep watermarks in")
        self.assertEqual(self.submit({"records": [exact]})[0], 0)
        state_path = next((self.state / "projects").iterdir())
        state = json.loads(state_path.read_text())
        now = int(time.time() * 1000)

        def stored(name: str, age_days: int, *days: str) -> dict[str, Any]:
            lines = [f"Decision: {name}", "Project: repo"]
            lines += [f"Confirmed: {', '.join(days)}"] if days else []
            created = now - age_days * 86_400_000
            content = "\n".join([*lines, "Evidence:", '- message m1: "quoted"'])
            return {"id": f"{PREFIX}record/x/{name}", "author_kind": "bot"} | {
                "content": content,
                "created_at": created,
            }

        def day(age: int) -> str:
            return time.strftime("%Y-%m-%d", time.gmtime(now / 1000 - age * 86400))

        records = [
            stored("newest once", 0),
            stored("confirmed often", 30, day(30), day(9), day(2), day(1)),
            stored("old once", 40),
        ]
        self.fake(chats=[], messages=MESSAGES + records)
        state["latest_record"] = records[0]["id"]
        state_path.write_text(json.dumps(state))
        text = self.hook()
        lines = [line for line in text.split("\n") if line.startswith("- Decision")]
        self.assertEqual(
            [line.split(" [")[0] for line in lines],
            [
                "- Decision: confirmed often (confirmed 4x)",
                "- Decision: newest once",
                "- Decision: old once",
            ],
        )
        self.assertIn("Project records (most confirmed first):", text)

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

    def stored(self) -> dict[str, Any]:
        data = json.loads((self.root / "fake.json").read_text())
        return {m["id"]: m for m in data["messages"] if m["id"].startswith(PREFIX)}

    def batches(self) -> list[Any]:
        return json.loads((self.root / "fake.json").read_text())["batches"]

    def accept(self, *records: dict[str, Any]) -> Any:
        status, output = self.submit({"records": list(records)})
        self.assertEqual((status, output["dropped"]), (0, []), output)
        return output

    def test_decisions_need_the_users_words(self) -> None:
        proposal = record("Use oxlint", "m4", "switching from Biome to oxlint")
        approved = {**proposal, "statement": "Use oxlint, approved"}
        approved["evidence"] = proposal["evidence"] + OXLINT["evidence"]
        noted = {**proposal, "kind": "fact", "statement": "oxlint was proposed"}
        preference = {**proposal, "kind": "preference", "statement": "Prefer oxlint"}
        status, output = self.submit(
            {"records": [proposal, approved, noted, preference]}
        )
        self.assertEqual((status, output["verified"]), (0, 2))
        dropped = {item["statement"]: item["reason"] for item in output["dropped"]}
        self.assertEqual(list(dropped), ["Use oxlint", "Prefer oxlint"])
        self.assertIn("must quote the user's own message", dropped["Use oxlint"])

    def test_supersession_chain_history_and_recall(self) -> None:
        [biome] = self.accept(BIOME)["written"]
        before = self.stored()[biome]
        output = self.accept(OXLINT)
        [oxlint] = output["current"]
        self.assertEqual(output["superseded"], [{"id": biome, "by": oxlint}])
        replaced = self.stored()[biome]
        self.assertEqual(replaced["created_at"], before["created_at"])
        self.assertEqual(
            replaced["content"].split("\n")[:3],
            [
                "Superseded decision: Lint with Biome",
                f"Superseded by: {oxlint} ({OXLINT_HEADING})",
                before["content"].split("\n")[1],
            ],
        )
        self.assertTrue(
            replaced["content"].endswith(before["content"].split("\n", 1)[1])
        )
        rewrite = [
            c
            for c in self.batches()[-1]["changes"]
            if c.get("id") == biome.removeprefix(PREFIX)
        ]
        self.assertEqual([c["kind"] for c in rewrite], ["recall:superseded"])
        self.assertIn(
            f"Supersedes: {biome} (Decision: Lint with Biome)",
            self.stored()[oxlint]["content"],
        )

        tsgolint = record(
            "Lint with oxlint and tsgolint type-aware rules",
            "m6",
            "type-aware rules through tsgolint",
            {"id": oxlint},
        )
        [current] = self.accept(tsgolint)["current"]
        text = self.hook()
        self.assertIn("Decision: Lint with oxlint and tsgolint type-aware rules", text)
        self.assertNotIn("Lint with Biome", text)
        self.assertNotIn("instead of Biome", text)

        result = self.run_script("record.py", b"", "--history", biome)
        chain = json.loads(result.stdout)
        self.assertEqual(chain["current"], [current])
        self.assertEqual([r["id"] for r in chain["records"]], [biome, oxlint, current])
        self.assertEqual(chain["records"][1]["superseded_by"], current)

        # Re-recording keeps replaced records replaced, in place and linked.
        count = len(self.batches())
        output = self.accept(BIOME, {**OXLINT, "supersedes": []})
        self.assertEqual((output["current"], output["superseded"]), ([], []))
        stored = self.stored()
        self.assertTrue(stored[biome]["content"].startswith("Superseded decision:"))
        self.assertIn(f"Superseded by: {current}", stored[oxlint]["content"])
        self.assertIn(f"Supersedes: {biome}", stored[oxlint]["content"])
        self.assertEqual(stored[biome]["created_at"], before["created_at"])
        kinds = {
            c["id"]: c["kind"] for c in self.batches()[count]["changes"] if "id" in c
        }
        self.assertEqual(kinds[biome.removeprefix(PREFIX)], "recall:superseded")
        self.accept(OXLINT)  # The original submission is idempotent too.
        self.assertTrue(self.stored()[oxlint]["content"].startswith("Superseded"))

    def test_supersession_rejections(self) -> None:
        self.accept(BIOME, OXLINT)  # One submission may supersede its own record.
        count = len(self.batches())
        quoted = ("m1", "keep watermarks in")
        records = [
            record("Self", *quoted, {"statement": "Self"}),
            record("Fork", "m5", "go with oxlint", {"statement": "Lint with Biome"}),
            {**BIOME, "supersedes": [{"statement": OXLINT["statement"]}]},
            record("Missing", *quoted, {"statement": "Never recorded"}),
            record("Elsewhere", *quoted, {"id": "record/other/0"}),
            record("Later", *quoted, {"statement": "Later target"}),
            record("Later target", "m2", "exact lookups"),
        ]
        status, output = self.submit({"records": records})
        self.assertEqual((status, output["verified"]), (0, 1), output)
        reasons = {item["statement"]: item["reason"] for item in output["dropped"]}
        self.assertIn("cannot supersede itself", reasons["Self"])
        self.assertIn("already superseded by", reasons["Fork"])
        self.assertIn("cycle", reasons["Lint with Biome"])
        self.assertIn("not found", reasons["Missing"])
        self.assertIn("records of this project", reasons["Elsewhere"])
        self.assertIn("list it first", reasons["Later"])
        self.assertEqual(len(self.batches()), count + 1)  # Only "Later target".
        both = {"id": "x", "statement": "y"}
        status, output = self.submit(
            {"records": [record("Bad", "m1", "keep watermarks in", both)]}
        )
        self.assertEqual(status, 1)
        self.assertIn("exactly one of id or statement", output["error"])


# A Claude Code session with decisions taken on 2026-10-28, user words and an
# assistant proposal kept apart.
SESSION = [
    ("user", "Lint the harness with Biome across the whole monorepo."),
    ("user", "Keep the harness decision memory in Mem0 for now."),
    ("user", "Run MinIO as the object store for celld artifacts."),
    ("assistant", "I suggest replacing Biome with oxlint; it is much faster."),
    ("user", "Agreed: switch to oxlint and remove Biome from the toolchain."),
    ("user", "Record notable harness decisions in Mash, not Mem0, with supersession."),
    ("user", "Shelve MinIO; celld does not need an object store yet."),
]


@unittest.skipUnless(os.environ.get("MASH_RECALL_TEST_BINARY"), "needs a built mash")
class RealMash(unittest.TestCase):
    """Record, supersede and recall through a real Mash and a disposable index."""

    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = self.root = Path(directory.name)
        sessions = root / "claude" / "project"
        sessions.mkdir(parents=True)
        rows = [
            {
                "type": role,
                "sessionId": "decisions",
                "uuid": f"u{n}",
                "cwd": str(root / "repo"),
                "timestamp": f"2026-10-28T09:{n:02}:00Z",
                "message": {"role": role, "content": text},
            }
            for n, (role, text) in enumerate(SESSION)
        ]
        lines = "".join(json.dumps(row) + "\n" for row in rows)
        (sessions / "decisions.jsonl").write_text(lines)
        (root / "config.toml").write_text(
            "version = 1\n[providers.claude-code]\n"
            f"roots = [{json.dumps(str(root / 'claude'))}]\n"
            "[views.build]\nrepresentations = 'lexical-only'\n"
            "[views.query]\nintent = 'exact-mention'\n"
        )
        real = os.environ["MASH_RECALL_TEST_BINARY"]
        options = f"--config {root}/config.toml --db {root}/mash.sqlite3"
        options += f" --models-dir {root}/no-models"
        self.mash = root / "mash"
        self.mash.write_text(f'#!/bin/sh\nexec {real} {options} "$@"\n')
        self.mash.chmod(0o755)
        self.repo = root / "repo"
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        git_remote = ["git", "-C", str(self.repo), "remote", "add", "origin", ORIGIN]
        subprocess.run(git_remote, check=True)
        self.env = {**os.environ, "MASH_BIN": str(self.mash)}
        self.env["MASH_RECALL_STATE_DIR"] = str(root / "state")
        self.call("index")
        [chat] = self.call("list-chats", "--local")["chats"]
        newest = chat["enrichment"]["timestamps"]["updated_at"]["message_id"]
        viewed = self.call("view", newest, "--context", str(len(SESSION)))
        self.ids = {m["content"]: m["id"] for m in viewed["messages"]}

    def call(self, *arguments: str) -> Any:
        result = subprocess.run(
            [str(self.mash), *arguments], capture_output=True, check=True
        )
        return json.loads(result.stdout.splitlines()[-1])

    def cite(self, kind: str, statement: str, *quotes: str, **extra: Any) -> Any:
        evidence = [
            {"message_id": next(v for k, v in self.ids.items() if quote in k)}
            | {"quote": quote}
            for quote in quotes
        ]
        return {"kind": kind, "statement": statement, "evidence": evidence, **extra}

    def submit(self, *records: Any) -> Any:
        stdin = json.dumps({"records": list(records)}).encode()
        command = [sys.executable, str(HOOKS / "record.py"), "--cwd", str(self.repo)]
        result = subprocess.run(command, input=stdin, capture_output=True, env=self.env)
        self.assertEqual(result.returncode, 0, result.stdout)
        return json.loads(result.stdout)

    def search(self, query: str, kind: str) -> list[str]:
        self.call("index")
        options = ["--local", "--unit", "messages", "--source", "mash-recall"]
        found = self.call("search", query, *options, "--kind", kind)
        return sorted(r["content"].split("\n")[0] for r in found["results"])

    def test_decisions_superseded_end_to_end(self) -> None:
        first = self.submit(
            self.cite("decision", "Lint with Biome", "Lint the harness with Biome"),
            self.cite(
                "decision",
                "Decision memory lives in Mem0",
                "Keep the harness decision memory in Mem0",
            ),
            self.cite(
                "decision",
                "MinIO stores celld artifacts",
                "Run MinIO as the object store",
            ),
            self.cite(
                "decision", "Adopt oxlint", "I suggest replacing Biome with oxlint"
            ),
        )
        self.assertEqual(first["verified"], 3)
        self.assertIn("user's own message", first["dropped"][0]["reason"])
        biome, mem0, minio = first["written"]
        updates = [
            self.cite(
                "decision",
                "Lint with oxlint instead of Biome",
                "I suggest replacing Biome with oxlint",
                "switch to oxlint and remove Biome",
                supersedes=[{"statement": "Lint with Biome"}],
            ),
            self.cite(
                "decision",
                "Record harness decisions in Mash, not Mem0",
                "Record notable harness decisions in Mash, not Mem0",
                supersedes=[{"id": mem0}],
            ),
            self.cite(
                "decision",
                "MinIO is shelved; celld needs no object store yet",
                "Shelve MinIO; celld does not need an object store yet.",
                supersedes=[{"id": minio}],
            ),
        ]
        second = self.submit(*updates)
        self.assertEqual(second["dropped"], [])
        replaced = {item["id"] for item in second["superseded"]}
        self.assertEqual(replaced, {biome, mem0, minio})
        self.assertEqual(self.submit(*updates)["written"], second["written"])

        payload = json.dumps({"session_id": "now", "cwd": str(self.repo)}).encode()
        command = [sys.executable, str(HOOKS / "session_start.py")]
        hook = subprocess.run(command, input=payload, capture_output=True, env=self.env)
        text = json.loads(hook.stdout)["hookSpecificOutput"]["additionalContext"]
        for statement in ("instead of Biome", "in Mash, not Mem0", "MinIO is shelved"):
            self.assertIn(statement, text)
        for statement in ("Lint with Biome", "lives in Mem0", "stores celld"):
            self.assertNotIn(statement, text)

        command = [sys.executable, str(HOOKS / "record.py"), "--history", biome]
        result = subprocess.run(command, capture_output=True, env=self.env)
        chain = json.loads(result.stdout)
        self.assertEqual(chain["current"], [second["written"][0]])
        self.assertEqual(len(chain["records"]), 2)

        self.assertEqual(
            self.search("Biome", "recall:decision"),
            ["Decision: Lint with oxlint instead of Biome"],
        )
        self.assertEqual(
            self.search("Biome", "recall:superseded"),
            ["Superseded decision: Lint with Biome"],
        )
        listed = self.call("list-chats", "--local", "--kind", "recall:project")
        self.assertIn(f"Project recall: {ORIGIN}", json.dumps(listed))


if __name__ == "__main__":
    unittest.main()
