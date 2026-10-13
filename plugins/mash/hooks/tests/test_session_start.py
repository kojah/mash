from __future__ import annotations

import json
import time
import unittest
from pathlib import Path

from support import MINUTE_MS, NOW_MS, Environment, chat

from mash_recall import client, project, recall, records, state

HELPER = Path("/plugin/hooks/record.py")


def payload(session_id: str, cwd: Path | str) -> bytes:
    return json.dumps(
        {"session_id": session_id, "cwd": str(cwd), "source": "startup"}
    ).encode()


class FailOpen(Environment):
    def assert_silent(self, result) -> None:
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")

    def test_missing_binary(self) -> None:
        self.environment["MASH_BIN"] = str(self.root / "absent")
        self.assert_silent(self.run_script("session_start.py", payload("s", self.root)))

    def test_malformed_input(self) -> None:
        self.assert_silent(self.run_script("session_start.py", b"{not json"))
        self.assert_silent(self.run_script("session_start.py", b"[]"))

    def test_mash_failure(self) -> None:
        repo = self.git_project()
        self.write_fixture(
            chats=[chat("other", NOW_MS, str(repo))], fail={"list-chats": 5}
        )
        self.assert_silent(self.run_script("session_start.py", payload("s", repo)))

    def test_missing_index_output(self) -> None:
        self.fixture.unlink()  # The fake cannot answer, like a missing database.
        self.assert_silent(
            self.run_script("session_start.py", payload("s", self.git_project()))
        )

    def test_deadline(self) -> None:
        repo = self.git_project()
        self.write_fixture(chats=[chat("other", NOW_MS, str(repo))], sleep=10)
        started = time.monotonic()
        result = self.run_script("session_start.py", payload("s", repo))
        self.assert_silent(result)
        self.assertLess(time.monotonic() - started, 6)

    def test_nonexistent_cwd(self) -> None:
        self.assert_silent(
            self.run_script("session_start.py", payload("s", self.root / "gone"))
        )

    def test_emits_hook_output(self) -> None:
        repo = self.git_project()
        now = int(time.time() * 1000)
        self.write_fixture(chats=[chat("other", now, str(repo), title="Prior work")])
        result = self.run_script("session_start.py", payload("s", repo / "sub"))
        self.assertEqual(result.returncode, 0)
        output = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "SessionStart")
        self.assertIn("Prior work", output["additionalContext"])


class ProjectResolution(Environment):
    def test_subdirectory_resolves_to_toplevel_and_origin(self) -> None:
        repo = self.git_project()
        resolved = project.resolve(str(repo / "sub"), 2.0)
        assert resolved is not None
        self.assertEqual(resolved.toplevel, str(repo))
        self.assertEqual(resolved.repository, "https://example.test/repo.git")

    def test_non_git_directory_is_its_own_project(self) -> None:
        plain = self.root / "plain"
        plain.mkdir()
        resolved = project.resolve(str(plain), 2.0)
        assert resolved is not None
        self.assertEqual(resolved, project.Project(str(plain), None))

    def test_ownership(self) -> None:
        owner = project.Project("/w/repo", "https://example.test/repo.git")

        def row(path: str | None, repository: str | None = None) -> client.Chat:
            decoded = client.decode_chat(chat("c", NOW_MS, path, repository))
            assert decoded is not None
            return decoded

        self.assertTrue(owner.owns(row("/w/repo")))
        self.assertTrue(owner.owns(row("/w/repo/sub")))
        # Another worktree or clone of the same origin belongs to the project.
        self.assertTrue(owner.owns(row("/elsewhere", "https://example.test/repo.git")))
        self.assertFalse(owner.owns(row("/w/repository")))
        self.assertFalse(owner.owns(row(None)))
        self.assertFalse(owner.owns(row("/w/other", "https://example.test/other.git")))

    def test_connector_containers_are_not_chats(self) -> None:
        row = {"chat_id": "x", "source": {"connector": "slack"}, "updated_at": 1}
        self.assertIsNone(client.decode_chat(row))


class Recall(Environment):
    def setUp(self) -> None:
        super().setUp()
        self.project = project.Project("/w/repo", "https://example.test/repo.git")
        self.mash = client.Mash(str(self.binary))

    def build(self, session_id: str = "current", budget_seconds: float = 10) -> str:
        return recall.build(
            self.mash,
            self.project,
            session_id,
            self.state,
            HELPER,
            NOW_MS,
            recall.Deadline(budget_seconds),
        )

    def scan(self, marks: dict[str, state.Watermark]) -> recall.Scan:
        return recall.scan(
            self.mash, self.project, "current", marks, NOW_MS, recall.Deadline(10)
        )

    def test_excludes_current_session_and_other_projects(self) -> None:
        self.write_fixture(
            chats=[
                chat("current", NOW_MS, "/w/repo"),
                chat("unrelated", NOW_MS - 1, "/w/other"),
                chat("earlier", NOW_MS - 2, "/w/repo/sub"),
            ]
        )
        result = self.scan({})
        ids = [item.chat_id for item, _ in result.pending] + [
            item.chat_id for item in result.recent
        ]
        self.assertEqual(ids, ["earlier"])
        self.assertNotIn("chat=current", self.build())

    def test_watermarks_select_pending_chats(self) -> None:
        week = 7 * 24 * 60 * MINUTE_MS
        self.write_fixture(
            chats=[
                chat("advanced", NOW_MS - 1 * MINUTE_MS, "/w/repo"),
                chat("unchanged", NOW_MS - 2 * MINUTE_MS, "/w/repo"),
                chat("new", NOW_MS - 3 * MINUTE_MS, "/w/repo"),
                chat("stale", NOW_MS - week - MINUTE_MS, "/w/repo"),
            ]
        )
        marks = {
            "advanced": state.Watermark(NOW_MS - 10 * MINUTE_MS, "m1"),
            "unchanged": state.Watermark(NOW_MS - 2 * MINUTE_MS, "m2"),
        }
        result = self.scan(marks)
        self.assertEqual(
            [(item.chat_id, mark) for item, mark in result.pending],
            [("advanced", marks["advanced"]), ("new", None)],
        )
        self.assertEqual(
            [item.chat_id for item in result.recent], ["unchanged", "stale"]
        )

    def test_pending_is_capped(self) -> None:
        self.write_fixture(
            chats=[chat(f"c{index}", NOW_MS - index, "/w/repo") for index in range(8)]
        )
        result = self.scan({})
        self.assertEqual(len(result.pending), recall.PENDING_CHATS)
        self.assertEqual(len(result.recent), recall.RECENT_CHATS)

    def test_pending_lines_and_instructions(self) -> None:
        self.write_fixture(chats=[chat("new", NOW_MS, "/w/repo", last="tail-id")])
        text = self.build()
        self.assertIn('view={"id":"tail-id","context":20}', text)
        self.assertIn("2-3 line recap", text)
        self.assertIn(f"python3 '{HELPER}' --cwd '/w/repo'", text)

    def test_no_activity_is_silent(self) -> None:
        self.assertEqual(self.build(), "")

    def test_budget(self) -> None:
        long_title = "x" * 5000
        self.write_fixture(
            chats=[
                chat(
                    f"c{index}-" + "y" * 80, NOW_MS - index, "/w/repo", title=long_title
                )
                for index in range(20)
            ]
        )
        text = self.build()
        self.assertTrue(text)
        self.assertLessEqual(len(text), recall.BUDGET)

    def test_fit_drops_sections_before_exceeding(self) -> None:
        text = recall.fit(["h"], [["S", "a" * 50, "b" * 50]], ["t"], 70)
        self.assertEqual(text, "h\nS\n" + "a" * 50 + "\nt")
        self.assertEqual(recall.fit(["h" * 100], [], [], 50), "")

    def test_records_come_first(self) -> None:
        owner = self.project
        record = records.Record(
            records.RecordKind.decision,
            "Use list-chats for recall",
            (records.Evidence("m1", "use list-chats"),),
        )
        anchor = records.anchor(records.record_id(owner, record))
        self.write_fixture(
            chats=[chat("recent", NOW_MS - 30 * 24 * 60 * MINUTE_MS, "/w/repo")],
            messages=[
                {
                    "id": anchor,
                    "content": records.content(owner, record),
                    "created_at": 5,
                }
            ],
        )
        project_state = state.ProjectState("/w/repo", owner.repository)
        project_state.latest_record = anchor
        state.save_project(self.state, owner.key, project_state)
        text = self.build()
        self.assertLess(
            text.index("Decision: Use list-chats for recall"), text.index("chat=recent")
        )
        self.assertIn(f"[view {anchor}]", text)

    def test_hook_does_not_write_state(self) -> None:
        self.write_fixture(chats=[chat("new", NOW_MS, "/w/repo")])
        self.build()
        self.assertFalse(self.state.exists())


if __name__ == "__main__":
    unittest.main()
