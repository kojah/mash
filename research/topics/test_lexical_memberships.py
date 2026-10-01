import unittest

import numpy as np

from lexical_memberships import eligibility, selection


class LexicalMembershipTests(unittest.TestCase):
    def test_df_is_one_vote_per_parent_not_token_count(self):
        counts = np.array([[100, 1], [0, 1], [0, 0], [0, 0]])
        np.testing.assert_array_equal(eligibility(counts, ['rare', 'shared'], 2, .5), [False, True])

    def test_upper_df_boundary_and_numeric_candidate_policy_are_explicit(self):
        counts = np.array([[1, 1, 1], [1, 1, 1], [0, 0, 0], [0, 0, 0]])
        np.testing.assert_array_equal(eligibility(counts, ['alpha', '429', 'api429'], 2, .5), [True, False, True])
        np.testing.assert_array_equal(eligibility(counts, ['alpha', '429', 'api429'], 2, .49), [False, False, False])

    def test_invalid_counts_and_bounds_fail_closed(self):
        for values, minimum, maximum in [([[-1]], 2, .4), ([[float('nan')]], 2, .4), ([[1]], 0, .4), ([[1]], 2, 0)]:
            with self.assertRaises(ValueError):
                eligibility(np.array(values), ['a'], minimum, maximum)

    def test_no_winner_below_gate_and_recall_precision_ties_are_deterministic(self):
        def result(tp, fp, recall):
            return {'metrics': {'tp': tp, 'fp': fp, 'precision': tp / (tp + fp) if tp + fp else None, 'recall': recall}}
        self.assertIsNone(selection({'a': result(19, 0, .9), 'b': result(0, 0, 0)}))
        rows = {'c': result(16, 4, .4), 'b': result(16, 4, .4), 'a': result(15, 5, .3)}
        self.assertEqual(selection(rows), 'b')


if __name__ == '__main__':
    unittest.main()
