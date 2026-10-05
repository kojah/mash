"""Micro subgroup counts, weighted coverage and complete build measurements."""

import unittest

from blind_turn_pairs import Label, Reference, Stratum, Verdict
from run_study_development import Prediction, Row
from study_analysis import Dimension, build_costs, fit_costs, slices
from study_artifacts import Cost, View
from summarize_turn_study import verify_rows
from turn_evaluation import Counts
from turn_study_corpus import Phase


class StudyAnalysis(unittest.TestCase):
    def test_reporting_rejects_missing_or_duplicate_configuration_thread_rows(self):
        counts = Counts(tp=1, fp=0, fn=0, tn=0, ambiguous=0)
        rows = tuple(
            Row(
                name="candidate",
                thread_index=index,
                provider="codex",
                node="xenia",
                prompted_turns=2,
                counts=counts,
                primary_counts=counts,
                seconds=0.1,
            )
            for index in (0, 1)
        )
        verify_rows(rows, frozenset((0, 1)), frozenset(("candidate",)))
        for invalid in (rows[:1], rows + rows[:1]):
            with self.assertRaises(ValueError):
                verify_rows(invalid, frozenset((0, 1)), frozenset(("candidate",)))

    def test_strata_preserve_negative_only_and_primary_overlap_difference(self):
        positive = Counts(tp=1, fp=0, fn=0, tn=0, ambiguous=0)
        negative = Counts(tp=0, fp=1, fn=0, tn=0, ambiguous=0)
        rejected = Counts(tp=0, fp=0, fn=0, tn=1, ambiguous=0)
        rows = tuple(
            Row(
                name="candidate",
                thread_index=index,
                provider="codex",
                node="xenia",
                prompted_turns=2,
                counts=counts,
                primary_counts=primary,
                seconds=0.01,
                predictions=(
                    Prediction(
                        pair_id=identity,
                        stratum=Stratum.DISTANT,
                        overlapping=True,
                        primary=bool(index == 0),
                    ),
                ),
            )
            for index, (identity, counts, primary) in enumerate(
                (("positive", positive, positive), ("negative", negative, rejected))
            )
        )
        labels = {
            identity: Label(
                pair_id=identity, verdict=verdict, note="Synthetic judgment"
            )
            for identity, verdict in (
                ("positive", Verdict.RELATED),
                ("negative", Verdict.UNRELATED),
            )
        }
        refs = {
            identity: Reference(
                pair_id=identity,
                thread_index=index,
                phase=Phase.DEVELOPMENT,
                a_turn_id="a",
                b_turn_id="b",
                stratum=Stratum.DISTANT,
            )
            for index, identity in enumerate(labels)
        }
        report = slices(rows, labels, refs)
        distant = next(
            value for value in report if value.dimension == Dimension.STRATUM
        )
        self.assertEqual((distant.counts.tp, distant.counts.fp), (1, 1))
        self.assertEqual(distant.counts.precision, 0.5)
        self.assertEqual(distant.primary_counts.fp, 0)
        self.assertEqual(distant.threads, 2)

    def test_coverage_weights_turns_and_controls_keep_unmeasured_coverage(self):
        counts = Counts(tp=0, fp=0, fn=0, tn=1, ambiguous=0)
        rows = tuple(
            Row(
                name="candidate",
                thread_index=i,
                provider="codex",
                node="xenia",
                prompted_turns=length,
                counts=counts,
                primary_counts=counts,
                seconds=1,
                assigned_turns=1,
                group_sizes=(2,),
            )
            for i, length in enumerate((2, 10))
        )
        metrics = fit_costs(rows)[0]
        self.assertEqual(metrics.assignment_coverage, 2 / 12)
        self.assertEqual(metrics.largest_group, 2)
        controls = tuple(
            row.model_copy(update={"assigned_turns": None}) for row in rows
        )
        self.assertIsNone(fit_costs(controls)[0].assignment_coverage)

    def test_build_costs_reject_incomplete_or_duplicate_thread_views(self):
        costs = tuple(
            Cost(
                thread_index=0,
                phase=Phase.DEVELOPMENT,
                view=view,
                seconds=10,
                input_units=3,
                clustering_points=2,
                new_vectors=1,
                input_tokens=50,
                discarded_tokens=5,
            )
            for view in View
        )
        self.assertEqual(len(build_costs(costs, frozenset((0,)))), 5)
        for invalid in (costs[:-1], costs + costs[:1]):
            with self.assertRaises(ValueError):
                build_costs(invalid, frozenset((0,)))


if __name__ == "__main__":
    unittest.main()
