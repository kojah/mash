"""Follow-up selection cannot hide long-history failures behind short histories."""

import unittest

from run_input_followup import CELLS, choose
from run_study_development import Row
from study_scoring import Summary
from turn_evaluation import Counts


def row(name: str, turns: int, counts: Counts) -> Row:
    return Row(
        name=name,
        thread_index=turns,
        provider="codex",
        node="test",
        prompted_turns=turns,
        counts=counts,
        primary_counts=counts,
        seconds=1,
    )


class FollowupSelection(unittest.TestCase):
    def test_grid_has_exactly_sixteen_unique_cells(self):
        self.assertEqual(len(CELLS), 16)
        self.assertEqual(len({cell.name for cell in CELLS}), 16)

    def test_overall_success_with_long_failure_cannot_advance(self):
        overall = Counts(tp=900, fp=50, fn=100, tn=50, ambiguous=0)
        long = Counts(tp=10, fp=10, fn=10, tn=10, ambiguous=0)
        baseline = Summary(
            name="chronological_4",
            counts=Counts(tp=400, fp=100, fn=400, tn=100, ambiguous=0),
            seconds=0,
        )
        summary = Summary(name="candidate", counts=overall, seconds=1)
        self.assertIsNone(
            choose((summary,), (row("candidate", 10, long),), baseline).selected
        )
        self.assertEqual(
            choose((summary,), (row("candidate", 10, overall),), baseline).selected,
            "candidate",
        )

    def test_ties_use_seconds_then_name_and_no_long_evidence_rejects(self):
        counts = Counts(tp=90, fp=5, fn=10, tn=5, ambiguous=0)
        baseline = Summary(
            name="chronological_4",
            counts=Counts(tp=40, fp=10, fn=40, tn=10, ambiguous=0),
            seconds=0,
        )
        candidates = tuple(
            Summary(name=name, counts=counts, seconds=seconds)
            for name, seconds in (("c", 2), ("b", 1), ("a", 1))
        )
        rows = tuple(row(summary.name, 10, counts) for summary in candidates)
        self.assertEqual(choose(candidates, rows, baseline).selected, "a")
        self.assertIsNone(choose(candidates, (), baseline).selected)


if __name__ == "__main__":
    unittest.main()
