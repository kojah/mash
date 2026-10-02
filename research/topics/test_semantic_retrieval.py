import unittest

import numpy as np

from semantic_retrieval import aggregate, rank_fusion


class SemanticRetrievalTests(unittest.TestCase):
    def test_source_weighted_direction_and_balanced_fields(self):
        x, y = np.eye(384)[:2]
        np.testing.assert_allclose(aggregate([x,y],[3,4])[:2], [.6,.8], atol=1e-12)
        np.testing.assert_allclose(aggregate([x,y],[1,1])[:2], [2**-.5]*2, atol=1e-12)
        np.testing.assert_array_equal(aggregate([x],[7]), x)

    def test_canceling_missing_and_malformed_fields_fail_explicitly(self):
        x = np.eye(384)[0]
        for vectors, weights in [([x,-x],[1,1]), ([x],[0]), ([x],[-1]),
                                  ([x*2],[1]), ([x],[np.nan]), ([],[])]:
            with self.assertRaises(ValueError): aggregate(vectors, weights)

    def test_fixed_fusion_full_rank_ties_and_no_score_scale_dependency(self):
        result = rank_fusion([3,2,1], [1,2,3])
        np.testing.assert_array_equal(result, [1/61+1/63, 2/62, 1/63+1/61])
        np.testing.assert_array_equal(result, rank_fusion([30,20,10], [-1,-.5,0]))
        np.testing.assert_array_equal(rank_fusion([0,0,0],[0,0,0]), [2/61,2/62,2/63])
        for left,right in [([],[]), ([1],[1,2]), ([np.nan],[1])]:
            with self.assertRaises(ValueError): rank_fusion(left,right)


if __name__ == '__main__':
    unittest.main()
