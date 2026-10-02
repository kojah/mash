import unittest
import numpy as np
from dual_lexical_evidence import fuse
from causal_lexical_context import rank


class DualEvidenceTests(unittest.TestCase):
    def test_each_source_identity_occurs_once_with_two_rank_contributions(self):
        value = fuse([3,2,1], [1,2,3], [0,1,2])
        self.assertEqual(value.tolist(), [1/61+1/63, 2/62, 1/63+1/61])
        self.assertEqual(rank(value, [0,1,2], np.array([True,False,False]))['top10_ordinals'], [0,2,1])

    def test_zero_ties_are_original_order_and_symmetric_views_match(self):
        value = fuse([0,0,0], [0,0,0], [3,1,2])
        self.assertEqual(value.tolist(), [2/63,2/61,2/62])
        self.assertTrue(np.array_equal(fuse([1,2,3],[2,3,1],[0,1,2]),fuse([2,3,1],[1,2,3],[0,1,2])))

    def test_full_list_retains_candidates_beyond_top_ten(self):
        a = np.arange(30, dtype=float);b = -a;ids = np.arange(30)
        value = fuse(a,b,ids)
        self.assertEqual(value[15],1/75+1/76)
        self.assertEqual(len(value),30)

    def test_malformed_alignment_and_duplicate_sources_fail_closed(self):
        for a,b,ids in [([],[],[]),([1],[1],[True]),([1],[1],[-1]),([1,2],[1,2],[0,0]),
                        ([1],[1,2],[0]),([np.nan],[1],[0]),([1],[np.inf],[0])]:
            with self.assertRaises(ValueError):fuse(a,b,ids)
