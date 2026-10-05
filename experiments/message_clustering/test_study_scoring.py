"""Registered bounds, distinct-turn support, negative-only evidence and stop rules."""

import unittest

import numpy as np

from blind_turn_pairs import Verdict
from run_study_development import bootstrap
from study_artifacts import View
from study_scoring import (
    CANDIDATES,
    Candidate,
    Family,
    JudgedPair,
    Summary,
    choose,
    combine,
    connections,
    fit,
    lexical_baselines,
    score,
)
from turn_evaluation import Counts


class StudyScoring(unittest.TestCase):
    def test_paired_bootstrap_is_deterministic_and_zero_when_no_predictions(self):
        counts = (
            Counts(tp=0, fp=0, fn=2, tn=5, ambiguous=1),
            Counts(tp=0, fp=0, fn=0, tn=10, ambiguous=0),
        )
        interval = bootstrap(counts, counts)
        self.assertEqual(interval, bootstrap(counts, counts))
        self.assertEqual(
            (interval.f1_low, interval.f1_high, interval.gain_low, interval.gain_high),
            (0.0, 0.0, 0.0, 0.0),
        )
        with self.assertRaises(ValueError):
            bootstrap(counts, counts[:1])

    def test_grid_is_exactly_sixty_unique_registered_settings(self):
        self.assertEqual(len(CANDIDATES), 60)
        self.assertEqual(len({c.name for c in CANDIDATES}), 60)
        for candidate in CANDIDATES:
            self.assertIn(candidate.cutoff, (0.65, 0.75, 0.85))
            self.assertEqual(
                candidate.radius is not None, candidate.family == Family.BIRCH
            )

    def test_repeated_passages_from_one_turn_cannot_form_retained_group(self):
        candidate = Candidate(view=View.PASSAGES, family=Family.COMPLETE, cutoff=0.75)
        result = fit(
            np.array([[1.0, 0.0], [1.0, 0.0], [0.0, 1.0]]), ("a", "a", "b"), candidate
        )
        self.assertEqual(result.group_sizes, ())
        self.assertEqual(result.overlapping, {"a": frozenset(), "b": frozenset()})
        supported = fit(
            np.array([[1.0, 0.0], [1.0, 0.0], [1.0, 0.0]]), ("a", "a", "b"), candidate
        )
        self.assertEqual(supported.group_sizes, (2,))
        pair = JudgedPair("pair", "a", "b", Verdict.RELATED)
        self.assertEqual(connections((pair,), supported.overlapping), {"pair": True})

    def test_negative_only_threads_reduce_micro_precision(self):
        positive = score((JudgedPair("p", "a", "b", Verdict.RELATED),), {"p": True})
        negative = score((JudgedPair("n", "c", "d", Verdict.UNRELATED),), {"n": True})
        total = combine((positive, negative))
        self.assertEqual((total.tp, total.fp, total.fn, total.tn), (1, 1, 0, 0))
        self.assertEqual(total.precision, 0.5)
        with self.assertRaises(ValueError):
            score((JudgedPair("p", "a", "b", Verdict.RELATED),), {})

    def test_baselines_distinguish_identical_text_from_chronological_position(self):
        pairs = (JudgedPair("far", "a", "c", Verdict.RELATED),)
        predictions = lexical_baselines(
            (
                "qdrant vector index",
                "unrelated medical condition",
                "qdrant vector index",
            ),
            ("a", "b", "c"),
            pairs,
        )
        self.assertTrue(predictions["tfidf_0.85"]["far"])
        self.assertFalse(predictions["chronological_2"]["far"])
        self.assertTrue(predictions["chronological_4"]["far"])
        self.assertFalse(predictions["no_cross_turn"]["far"])

    def test_decision_stops_at_good_baseline_or_no_accepted_method(self):
        good = Counts(tp=90, fp=5, fn=10, tn=200, ambiguous=0)
        weak = Counts(tp=50, fp=20, fn=50, tn=200, ambiguous=0)
        baseline = Summary(name="simple", counts=good, seconds=1)
        candidate = Summary(name="complex", counts=good, seconds=2)
        decision = choose((candidate,), (baseline,))
        self.assertEqual(decision.selected, "simple")
        self.assertFalse(decision.requires_gain)
        rejected = choose(
            (Summary(name="weak", counts=weak, seconds=1),),
            (Summary(name="baseline", counts=weak, seconds=1),),
        )
        self.assertIsNone(rejected.selected)
        improved = choose(
            (candidate,), (Summary(name="baseline", counts=weak, seconds=1),)
        )
        self.assertEqual(improved.selected, "complex")
        self.assertTrue(improved.requires_gain)


if __name__ == "__main__":
    unittest.main()
