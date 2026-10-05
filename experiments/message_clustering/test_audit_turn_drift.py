"""Counterfactuals retain embeddings/order and distinguish later-turn effects."""

import unittest

import numpy as np

from audit_turn_drift import Length, Observation, prefix_prediction, summarize
from blind_turn_pairs import Verdict
from study_artifacts import View
from study_scoring import Candidate, Family, JudgedPair, connections, fit


class DriftAudit(unittest.TestCase):
    def test_later_turns_can_break_identical_pair_without_changing_embeddings(self):
        vectors = np.array([[1.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, 1.0], [0.0, 1.0]])
        owners = ("a", "b", "c", "d", "e")
        candidate = Candidate(
            view=View.SEPARATE, family=Family.BIRCH, radius=0.75, cutoff=0.65
        )
        pair = JudgedPair("pair", "a", "b", Verdict.RELATED)
        before = vectors.copy()
        prefix, primary, stop = prefix_prediction(vectors, owners, pair, candidate)
        full = fit(vectors, owners, candidate)
        self.assertTrue(prefix)
        self.assertTrue(primary)
        self.assertEqual(stop, 2)
        self.assertFalse(connections((pair,), full.overlapping)[pair.id])
        self.assertTrue(connections((pair,), full.primary)[pair.id])
        np.testing.assert_array_equal(vectors, before)

    def test_summary_excludes_ambiguous_and_no_future_pairs_from_drift_counts(self):
        base = Observation(
            name="candidate",
            thread_index=0,
            prompted_turns=5,
            pair_id="p",
            verdict=Verdict.RELATED,
            cosine=0.5,
            prefix_turns=2,
            full=False,
            prefix=True,
            full_primary=True,
            prefix_primary=True,
        )
        no_future = base.model_copy(update={"pair_id": "q", "prefix_turns": 5})
        ambiguous = base.model_copy(
            update={"pair_id": "r", "verdict": Verdict.AMBIGUOUS}
        )
        negative = base.model_copy(
            update={
                "pair_id": "s",
                "verdict": Verdict.UNRELATED,
                "cosine": 0.1,
                "full": True,
                "prefix": False,
            }
        )
        result = summarize(
            (base, no_future, ambiguous, negative), "candidate", Length.ALL
        )
        self.assertEqual(
            (result.pairs, result.with_later_turns, result.changed), (3, 2, 2)
        )
        self.assertEqual(result.corrected_by_removing_later, 2)
        self.assertEqual(result.worsened_by_removing_later, 0)
        self.assertEqual(result.pair_cosine_auc, 1.0)


if __name__ == "__main__":
    unittest.main()
