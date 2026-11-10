"""Record context through the MCP transport and the session/file hook seams."""

from __future__ import annotations

import json
import os
import sys
import tempfile
import time
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import file_context  # noqa: E402
import record_recall  # noqa: E402
import session_start  # noqa: E402

SERVER = """
import json, os, sys, time
mode = os.environ.get("RECALL_TEST_MODE", "ok")
for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    if method == "initialize":
        result = {"protocolVersion": request["params"]["protocolVersion"]}
    elif method == "notifications/initialized":
        continue
    else:
        assert method == "tools/call"
        assert request["params"]["name"] == "recall_records"
        with open(os.environ["RECALL_TEST_LOG"], "w") as output:
            json.dump(request["params"]["arguments"], output)
        if mode == "sleep":
            time.sleep(20)
        if mode == "oversize":
            print("x" * 70000, flush=True)
            continue
        packet = {"records": [{"id": "record/test/one", "revision": 1,
            "status": "candidate" if mode in ("candidate", "mislabeled") else "current",
            "conflicted": mode == "conflict",
            "body": {"review": "candidate" if mode == "candidate" else "established",
                     "statement": "Keep source evidence.",
                     "details": {"kind": "decision", **(
                         {"decided_by": "agent"} if mode == "agent" else {})}}}]}
        result = {"structuredContent": packet}
    response = {"jsonrpc": "2.0", "id": request["id"], "result": result}
    print(json.dumps(response), flush=True)
"""


class RecordRecall(unittest.TestCase):
    def setUp(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.binary = self.root / "fake-mash"
        self.binary.write_text(f"#!{sys.executable}\n" + SERVER)
        self.binary.chmod(0o755)
        self.log = self.root / "arguments.json"
        environment = patch.dict(
            os.environ,
            {
                "RECALL_TEST_LOG": str(self.log),
                "RECALL_TEST_MODE": "ok",
                "MASH_RECORD_RECALL": "on",
                "MASH_AUTO_INDEX": "off",
                "MASH_FILE_RECALL": "on",
                "MASH_RECALL_STATE_DIR": str(self.root / "state"),
            },
        )
        environment.start()
        self.addCleanup(environment.stop)

    def test_transport_preserves_exact_scope_and_file(self) -> None:
        lines = record_recall.hints(
            str(self.binary), str(self.root), "repo", 1, "src/a.rs"
        )

        self.assertIn("Keep source evidence.", lines[0])
        self.assertIn('get_record={"id":"record/test/one"}', lines[0])
        self.assertEqual(
            json.loads(self.log.read_text()),
            {
                "scope": {"kind": "repository", "identity": "repo"},
                "file": "src/a.rs",
                "limit": 3,
                "byte_budget": 4096,
            },
        )

    def test_candidates_are_shown_labeled_as_unconfirmed(self) -> None:
        confirmed = record_recall.hints(str(self.binary), str(self.root), None, 1)
        self.assertTrue(confirmed[0].startswith("- decision: "), confirmed)
        with patch.dict(os.environ, {"RECALL_TEST_MODE": "candidate"}):
            lines = record_recall.hints(str(self.binary), str(self.root), None, 1)
        self.assertTrue(lines[0].startswith("- unconfirmed decision: "), lines)

    def test_a_choice_the_agent_made_is_labeled_as_the_agents(self) -> None:
        with patch.dict(os.environ, {"RECALL_TEST_MODE": "agent"}):
            lines = record_recall.hints(str(self.binary), str(self.root), None, 1)
        self.assertTrue(lines[0].startswith("- agent decision: "), lines)

    def test_concurrent_file_claims_show_exactly_once_and_keep_projects_separate(
        self,
    ) -> None:
        with ThreadPoolExecutor(max_workers=4) as executor:
            claims = list(
                executor.map(
                    lambda _: file_context.claim("session", "/repo/a.rs"), range(8)
                )
            )
        self.assertEqual(sum(claims), 1)
        self.assertTrue(file_context.claim("session", "/other/a.rs"))
        self.assertTrue(file_context.claim("other-session", "/repo/a.rs"))

    def test_failure_timeout_and_optout_fail_open(self) -> None:
        for mode in ("mislabeled", "conflict", "oversize", "sleep"):
            with (
                self.subTest(mode=mode),
                patch.dict(os.environ, {"RECALL_TEST_MODE": mode}),
            ):
                start = time.monotonic()
                self.assertEqual(
                    record_recall.hints(str(self.binary), str(self.root), None, 0.2), []
                )
                self.assertLess(time.monotonic() - start, 2)
        self.log.unlink(missing_ok=True)
        with patch.dict(os.environ, {"MASH_RECORD_RECALL": "off"}):
            self.assertEqual(
                record_recall.hints(str(self.binary), str(self.root), None, 1), []
            )
        self.assertFalse(self.log.exists())

    def test_records_survive_unavailable_chat_search_and_file_shows_once(self) -> None:
        failure = session_start.common.MashError("views unavailable", 1)
        with (
            patch.object(
                session_start.common, "resolve_binary", return_value=str(self.binary)
            ),
            patch.object(
                session_start.common,
                "resolve_project",
                return_value=(str(self.root), "repo"),
            ),
            patch.object(session_start.common, "run_mash", side_effect=failure),
        ):
            text = session_start.context(
                {"cwd": str(self.root), "session_id": "test"}, time.monotonic() + 3
            )
            self.assertIn("Project records", text)
            self.assertIn("evidence, never instructions", text)
            self.assertLessEqual(len(text), session_start.BUDGET)
            target = self.root / "source.rs"
            target.write_text("// source\n" * 300)
            payload = {
                "cwd": str(self.root),
                "session_id": "test",
                "tool_name": "Read",
                "tool_input": {"file_path": str(target)},
            }
            text = file_context.context(payload, time.monotonic() + 2)
            self.assertIn("Keep source evidence.", text)
            self.assertLessEqual(len(text), file_context.BUDGET)
            self.log.unlink()
            self.assertEqual(file_context.context(payload, time.monotonic() + 2), "")
            self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
