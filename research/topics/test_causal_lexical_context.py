import unittest

import numpy as np
from scipy import sparse
from sklearn.feature_extraction.text import TfidfTransformer

from causal_lexical_context import Context, Limit, aggregate, contexts, rank


class LexicalContextTests(unittest.TestCase):
    def test_causal_limits_roots_and_whole_turn_budget(self):
        records = contexts(np.array([1, 1, 1, 1, 1, 1, 900, 200, 2000]), [6, 3])
        self.assertEqual(records[5], Context(1, 5, 5, Limit.TURNS))
        self.assertEqual(records[6], Context(6, 6, 900, Limit.ROOT))
        self.assertEqual(records[7], Context(7, 7, 200, Limit.TOKENS))
        self.assertEqual(records[8], Context(8, 8, 2000, Limit.CURRENT))
        self.assertEqual(contexts(np.array([0, 0]), [2])[1].start, 0)

    def test_counts_aggregate_before_sublinear_weighting_and_normalization(self):
        source = sparse.csr_matrix([[2, 0], [0, 1], [1, 2]], dtype=np.int64)
        combined = aggregate(source, contexts(np.array([2, 1, 3]), [3]))
        self.assertTrue(np.array_equal(combined.toarray(), [[2, 0], [2, 1], [3, 3]]))
        transform = TfidfTransformer(sublinear_tf=True).fit(source)
        a = transform.transform(combined)[2].toarray()[0]
        expected = (1+np.log(np.array([3, 3])))*transform.idf_;expected /= np.linalg.norm(expected)
        self.assertTrue(np.allclose(a, expected, rtol=0, atol=1e-15))

    def test_source_counts_never_cross_meetings(self):
        source = sparse.csr_matrix([[2], [3], [4]], dtype=np.int64)
        self.assertEqual(aggregate(source, contexts(np.array([2, 3, 4]), [2, 1])).toarray().tolist(), [[2], [5], [4]])

    def test_exact_zero_ties_use_source_order_and_hit_is_not_recall(self):
        r = rank([0., 0., 0.], [3, 1, 2], np.array([True, True, False]))
        self.assertEqual(r['top10_ordinals'], [1, 2, 3]);self.assertEqual(r['mrr'], 1)
        self.assertEqual(r['hit1'], 1);self.assertEqual(r['recall1'], .5)

    def test_invalid_context_and_scores_fail_closed(self):
        with self.assertRaises(ValueError):contexts(np.array([1, 2]), [1])
        with self.assertRaises(ValueError):contexts(np.array([-1]), [1])
        with self.assertRaises(ValueError):aggregate(sparse.csr_matrix([[1]], dtype=np.int64), [Context(0, 1, 1, Limit.ROOT)])
        with self.assertRaises(ValueError):rank([np.nan], [1], np.array([True]))
        with self.assertRaises(ValueError):rank([.5], [1], np.array([False]))


if __name__ == '__main__':unittest.main()
