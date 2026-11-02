"""Record confirmation, strength ranking and duplicate detection, in process."""

from __future__ import annotations

import sys
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import recall_common as common  # noqa: E402
import record  # noqa: E402

PREFIX = common.ENTRY_PREFIX
DAY = 24 * 3600 * 1000
NOW = 1_793_000_000_000  # 2026-10-26


def stored(statement: str, *days: str, kind: str = "Decision") -> record.Stored:
    lines = [f"{kind}: {statement}", "Project: repo"]
    if days:
        lines.append(common.CONFIRMED + ", ".join(days))
    content = "\n".join([*lines, "Evidence:", '- message m1: "quoted words"'])
    return record.Stored(f"record/key/{abs(hash(statement))}", content, NOW - DAY)


def submitted(statement: str, kind: str = "decision") -> dict:
    evidence = [{"message_id": "m1", "quote": "quoted words"}]
    return {"kind": kind, "statement": statement, "evidence": evidence}


class Strength(unittest.TestCase):
    def test_confirmations_fall_back_to_the_record_time(self) -> None:
        self.assertEqual(common.confirmations(stored("A").content, NOW), ["2026-10-26"])
        dated = stored("A", "2026-10-01", "2026-10-20").content
        self.assertEqual(common.confirmations(dated, NOW), ["2026-10-01", "2026-10-20"])

    def test_repeated_confirmation_outranks_one_recent_record(self) -> None:
        once_today = common.strength(["2026-10-26"], NOW)
        often = common.strength(["2026-10-12", "2026-10-19", "2026-10-25"], NOW)
        once_old = common.strength(["2026-09-01"], NOW)
        self.assertGreater(often, once_today)
        self.assertGreater(once_today, once_old)
        self.assertEqual(common.strength(["not a day"], NOW), 0.0)


class Duplicates(unittest.TestCase):
    def test_restatement_rerecords_the_stored_statement(self) -> None:
        existing = [stored("Lint with oxlint, not Biome.")]
        mine = submitted("lint with OXLINT not biome")
        self.assertEqual(record.duplicates([mine], existing), [])
        self.assertEqual(mine["statement"], "Lint with oxlint, not Biome.")
        key = "key"
        self.assertEqual(
            record.record_id(key, mine),
            record.record_id(key, submitted("Lint with oxlint, not Biome.")),
        )

    def test_rewording_is_reported_not_merged(self) -> None:
        existing = [
            stored("Keep recall watermarks in the Mash data directory"),
            stored("Use exact cosine search for local vectors"),
            stored("Deploy only at agreed release milestones"),
        ]
        mine = submitted("Keep the recall watermarks in the Mash data directory")
        [similar] = record.duplicates([mine], existing)
        self.assertEqual(similar["similar_to"], PREFIX + existing[0].id)
        self.assertEqual(
            similar["heading"],
            "Decision: Keep recall watermarks in the Mash data directory",
        )
        self.assertGreaterEqual(similar["score"], record.SIMILAR_COSINE)
        self.assertEqual(mine["statement"], submitted(mine["statement"])["statement"])

    def test_numbers_kinds_and_unrelated_statements_stay_distinct(self) -> None:
        existing = [stored("Set the celld lease TTL to 120 seconds")]
        changed = submitted("Set the celld lease TTL to 60 seconds")
        fact = submitted("Set the celld lease TTL to 120 seconds", kind="fact")
        unrelated = submitted("Prefer Bun for the harness runtime")
        self.assertEqual(record.duplicates([changed, fact, unrelated], existing), [])
        self.assertEqual(fact["statement"], "Set the celld lease TTL to 120 seconds")


class Confirmation(unittest.TestCase):
    def content(self, previous: record.Stored | None) -> str:
        mine = submitted("Lint with oxlint")
        rid = record.record_id("key", mine)
        accepted = [record.Accepted(mine, rid, [])]
        stored_by_id: dict[str, record.Stored | None] = {rid: previous}
        changes = record.changes("key", "/repo", None, accepted, [], {}, stored_by_id)
        return changes[-1]["content"]

    def test_records_list_their_days_and_rerecording_adds_today(self) -> None:
        today = common.day(int(time.time() * 1000))
        self.assertIn(f"\n{common.CONFIRMED}{today}\nEvidence:", self.content(None))
        old = [f"2026-09-{n:02}" for n in range(1, 13)]
        previous = stored("Lint with oxlint", *old)
        line = self.content(previous).split("\n")[2]
        days = line[len(common.CONFIRMED) :].split(", ")
        self.assertEqual(days, [*old[-(common.CONFIRMATIONS - 1) :], today])
        again = stored("Lint with oxlint", "2026-09-01", today)
        line = self.content(again).split("\n")[2]
        self.assertEqual(line, f"{common.CONFIRMED}2026-09-01, {today}")


class Paths(unittest.TestCase):
    def test_path_keys_ignore_slash_style_and_windows_drive_case(self) -> None:
        self.assertEqual(
            common.path_key("C:\\Users\\Ada\\repo\\"),
            common.path_key("c:/users/ada/repo"),
        )
        self.assertEqual(common.path_key("/home/ada/Repo/"), "/home/ada/Repo")
        self.assertNotEqual(common.path_key("/home/ada/Repo"), "/home/ada/repo")


if __name__ == "__main__":
    unittest.main()
