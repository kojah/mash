"""Guard descriptive threshold scans against favourable tie splitting."""

import unittest

import numpy as np

from blind_turn_pairs import Verdict
from representation_diagnostic import Observation, normalize, summarize


class DiagnosticTests(unittest.TestCase):
    def test_ties_cannot_be_split(self) -> None:
        items = tuple(
            Observation(
                view="raw",
                thread_index=40,
                pair_id=str(i),
                verdict=verdict,
                turns=10,
                distance=5,
                similarity=0.8,
                comparisons=1,
            )
            for i, verdict in enumerate((Verdict.RELATED, Verdict.UNRELATED))
        )
        result = summarize(items, "raw", "long")
        self.assertEqual(result.auc, 0.5)
        self.assertEqual(result.optimistic_recall_at_precision_85, 0)
        self.assertEqual(summarize(items, "raw", "short").positives, 0)

    def test_normalization_rejects_invalid_vectors(self) -> None:
        np.testing.assert_allclose(normalize(np.array([[3.0, 4.0]])), [[0.6, 0.8]])
        for vectors in (np.zeros((1, 2)), np.array([[float("nan"), 1.0]])):
            with self.assertRaises(ValueError):
                normalize(vectors)


if __name__ == "__main__":
    unittest.main()
