from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import Any

from support import Environment

from mash_recall import client, project, records, state

MESSAGES = [
    {
        "id": "m1",
        "content": "We decided to keep watermarks under the Mash data directory.",
        "created_at": 1_000,
    },
    {
        "id": "m2",
        "content": "I prefer exact lexical lookups over model inference here.",
        "created_at": 2_000,
    },
    {"id": "m3", "content": "Done.", "created_at": 3_000},
]


def record(kind: str, statement: str, *evidence: tuple[str, str]) -> dict[str, object]:
    return {
        "kind": kind,
        "statement": statement,
        "evidence": [{"message_id": m, "quote": q} for m, q in evidence],
    }


EXACT = record(
    "decision",
    "Watermarks live in the Mash data directory.",
    ("m1", "keep watermarks under the Mash data directory"),
)
PARAPHRASE = record(
    "preference",
    "Prefer lexical lookups.",
    ("m2", "prefers lexical lookups rather than inference"),
)


class Helper(Environment):
    def setUp(self) -> None:
        super().setUp()
        self.repo = self.git_project()
        self.write_fixture(messages=MESSAGES)
        self.project = project.Project(str(self.repo), "https://example.test/repo.git")

    def submit(self, submission: object, *arguments: str) -> tuple[int, Any]:
        result = self.run_script(
            "record.py",
            json.dumps(submission).encode(),
            "--cwd",
            str(self.repo / "sub"),
            *arguments,
        )
        return result.returncode, json.loads(result.stdout)

    def project_state(self) -> state.ProjectState:
        return state.load_project(
            self.state, self.project.key, str(self.repo), self.project.repository
        )

    def test_paraphrase_is_dropped_and_exact_quote_written(self) -> None:
        status, output = self.submit(
            {
                "records": [EXACT, PARAPHRASE],
                "covered": [{"chat_id": "chat-a", "message_id": "m2"}],
            }
        )
        self.assertEqual(status, 0, output)
        self.assertEqual(output["verified"], 1)
        self.assertEqual(output["dropped"][0]["statement"], "Prefer lexical lookups.")
        batches = self.read_fixture()["batches"]
        self.assertEqual(len(batches), 1)
        batch = batches[0]
        self.assertEqual(batch["source_id"], "mash-recall")
        self.assertEqual(batch["expected_revision"], 0)
        entries = [
            change for change in batch["changes"] if change.get("role") == "entry"
        ]
        self.assertEqual(len(entries), 1)
        entry = entries[0]
        self.assertEqual(entry["kind"], "recall:decision")
        self.assertEqual(entry["parent"], f"project/{self.project.key}")
        self.assertEqual(entry["metadata"]["project"], str(self.repo))
        self.assertEqual(
            entry["metadata"]["evidence"],
            [
                {
                    "message_id": "m1",
                    "quote": "keep watermarks under the Mash data directory",
                }
            ],
        )
        self.assertIn("message m1", entry["content"])
        self.assertEqual(
            self.project_state().latest_record, records.anchor(entry["id"])
        )

    def test_quote_rules(self) -> None:
        def fetch(message_id: str) -> client.Message | None:
            for message in MESSAGES:
                if message["id"] == message_id:
                    return client.Message(message_id, str(message["content"]), 1)
            return None

        def problem(message_id: str, quote: str) -> str | None:
            return records.quote_problem(records.Evidence(message_id, quote), fetch)

        self.assertIsNone(problem("m1", "keep watermarks under"))
        self.assertIn("exact substring", problem("m1", "Keep watermarks under") or "")
        self.assertIn("exact substring", problem("m2", "prefer exact  lexical") or "")
        self.assertIn("shorter", problem("m3", "Done.") or "")
        self.assertIn("not found", problem("missing", "keep watermarks under") or "")

    def test_any_failing_quote_drops_the_record(self) -> None:
        mixed = record(
            "fact",
            "Mixed evidence.",
            ("m1", "keep watermarks under the Mash"),
            ("m2", "I prefer model inference"),
        )
        status, output = self.submit({"records": [mixed], "covered": []})
        self.assertEqual((status, output["verified"], output["written"]), (0, 0, []))
        self.assertEqual(self.read_fixture()["batches"], [])

    def test_watermarks_advance_only_for_covered_chats(self) -> None:
        status, output = self.submit(
            {"records": [], "covered": [{"chat_id": "chat-a", "message_id": "m2"}]}
        )
        self.assertEqual((status, output["advanced"]), (0, ["chat-a"]))
        marks = self.project_state().watermarks
        self.assertEqual(marks, {"chat-a": state.Watermark(2_000, "m2")})
        # An older message never moves a watermark backwards.
        status, output = self.submit(
            {"records": [], "covered": [{"chat_id": "chat-a", "message_id": "m1"}]}
        )
        self.assertEqual((status, output["advanced"]), (0, []))
        self.assertEqual(self.project_state().watermarks["chat-a"].updated_at, 2_000)

    def test_unreadable_cover_does_not_advance(self) -> None:
        status, output = self.submit(
            {"records": [], "covered": [{"chat_id": "chat-b", "message_id": "nope"}]}
        )
        self.assertEqual((status, output["uncovered"]), (0, ["chat-b"]))
        self.assertEqual(self.project_state().watermarks, {})

    def test_failed_delivery_does_not_advance(self) -> None:
        data = self.read_fixture()
        data["fail"] = {"index": 5}
        self.fixture.write_text(json.dumps(data))
        status, output = self.submit(
            {"records": [EXACT], "covered": [{"chat_id": "chat-a", "message_id": "m2"}]}
        )
        self.assertEqual(status, 1, output)
        self.assertEqual(self.project_state().watermarks, {})
        # The batch stays pending and is resent unchanged before new work.
        pending = state.load_source(self.state).pending
        self.assertIsNotNone(pending)
        data = self.read_fixture()
        data["fail"] = None
        self.fixture.write_text(json.dumps(data))
        status, output = self.submit(
            {"records": [], "covered": [{"chat_id": "chat-a", "message_id": "m2"}]}
        )
        self.assertEqual(status, 0, output)
        assert pending is not None
        self.assertEqual(
            [batch["batch_id"] for batch in self.read_fixture()["batches"]],
            [pending["batch_id"]],
        )
        self.assertEqual(state.load_source(self.state).revision, 1)

    def test_revisions_follow_receipts(self) -> None:
        for statement in ("First fact here.", "Second fact here."):
            entry = record("fact", statement, ("m1", "keep watermarks under the Mash"))
            status, output = self.submit({"records": [entry], "covered": []})
            self.assertEqual(status, 0, output)
        revisions = [
            batch["expected_revision"] for batch in self.read_fixture()["batches"]
        ]
        self.assertEqual(revisions, [0, 1])

    def test_replaced_database_restarts_revision(self) -> None:
        state.save_source(self.state, state.SourceState(revision=7))
        status, output = self.submit({"records": [EXACT], "covered": []})
        self.assertEqual(status, 0, output)
        self.assertEqual(self.read_fixture()["batches"][0]["expected_revision"], 0)

    def test_check_writes_nothing(self) -> None:
        status, output = self.submit(
            {"records": [EXACT], "covered": [{"chat_id": "c", "message_id": "m1"}]},
            "--check",
        )
        self.assertEqual((status, output["verified"]), (0, 1))
        self.assertEqual(self.read_fixture()["batches"], [])
        self.assertFalse(self.state.exists())

    def test_invalid_input(self) -> None:
        for submission in (
            {"records": [{"kind": "rumour", "statement": "x", "evidence": []}]},
            {"records": [record("fact", "No evidence.")]},
            {"extra": True},
        ):
            status, output = self.submit(submission)
            self.assertEqual(status, 1)
            self.assertIn("invalid input", output["error"])


@unittest.skipUnless(
    os.environ.get("MASH_RECALL_TEST_BINARY"),
    "set MASH_RECALL_TEST_BINARY to a built mash to exercise real silver",
)
class RealMash(unittest.TestCase):
    """Deliver through a real `mash index --connector-batch` into a disposable index."""

    def test_records_reach_silver_and_view(self) -> None:
        binary = os.environ["MASH_RECALL_TEST_BINARY"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "config.toml"
            config.write_text("version = 1\n[providers]\n")
            database = root / "mash.sqlite3"
            wrapper = root / "mash"
            wrapper.write_text(
                f'#!/bin/sh\nexec {binary} --config {config} --db {database} "$@"\n'
            )
            wrapper.chmod(0o755)
            mash = client.Mash(str(wrapper))
            owner = project.Project("/w/repo", "https://example.test/repo.git")
            source_state = state.SourceState()
            first = records.Record(
                records.RecordKind.decision,
                "First",
                (records.Evidence("m1", "quoted text"),),
            )
            second = records.Record(
                records.RecordKind.open_question,
                "Second",
                (records.Evidence("m2", "more text"),),
            )
            import record as helper

            helper.deliver(
                mash,
                root,
                source_state,
                helper.batch(
                    source_state, records.changes(owner, [first, second], (), 1_000)
                ),
            )
            self.assertEqual(source_state.revision, 1)
            newest = records.anchor(records.record_id(owner, second))
            viewed = mash.view(newest, 12, 120)  # A fresh index initializes slowly.
            self.assertEqual(
                [message.content.split("\n")[0] for message in viewed],
                ["Decision: First", "Open question: Second"],
            )
            listed = subprocess.run(
                [str(wrapper), "list-chats", "--local", "--kind", records.PROJECT_KIND],
                capture_output=True,
                check=True,
            )
            self.assertIn(
                b"Project recall: https://example.test/repo.git", listed.stdout
            )


if __name__ == "__main__":
    unittest.main()
