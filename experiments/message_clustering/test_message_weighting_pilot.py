import unittest

import numpy as np

from message_weighting_pilot import Corpus, rank


class PilotContracts(unittest.TestCase):
    def test_multi_target_ranking_ties_and_recall(self):
        result = rank(np.array([0.8, 0.8, 0.1]), ("a", "b", "c"), ("b", "c"), 0)
        self.assertEqual(result.target_rank, 2)
        self.assertEqual(result.relevant_at_five, 1)
        self.assertEqual(result.top_five, ("a", "b", "c"))

    def test_unknown_targets_are_rejected(self):
        with self.assertRaises(ValueError):
            Corpus.model_validate(
                {
                    "version": 1,
                    "documents": [{"id": "a", "raw": "hello", "cleaned": "hello"}],
                    "queries": [{"text": "query", "relevant": ["other"]}],
                }
            )


if __name__ == "__main__":
    unittest.main()
