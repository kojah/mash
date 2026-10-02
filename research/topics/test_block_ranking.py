import unittest

import numpy as np

from block_ranking import group,maximum,metrics
from return_ranking import fusion,rank


class BlockRankingTests(unittest.TestCase):
    def test_one_owned_result_and_conditional_mixed_gain(self):
        groups=group(np.array([4,4,9]),np.array([8,12,20]),np.array([True,False,True]))
        self.assertEqual(groups.ordinals.tolist(),[8,20]);self.assertEqual(groups.gains.tolist(),[.5,1.])
        scores=maximum(np.array([.9,.8,.7]),groups);result=metrics(scores,groups)
        self.assertEqual(result['returned'],2);self.assertEqual(result['top10_blocks'],[4,9])
        self.assertEqual(result['ap'],1.)
        expected=(.5+1/np.log2(3))/(1+.5/np.log2(3))
        self.assertAlmostEqual(result['ndcg10'],expected)

    def test_fusion_deduplicates_branch_votes_before_ranking(self):
        groups=group(np.array([0,0,1]),np.array([0,1,2]),np.array([False,False,True]))
        dense=maximum(np.array([1.,.9,.8]),groups);lexical=maximum(np.array([.8,.7,1.]),groups)
        fused=fusion(dense,lexical,groups.ordinals)
        self.assertEqual(fused[0],fused[1]);self.assertEqual(rank(fused,groups.ordinals).tolist(),[0,1])
        result=metrics(np.zeros(2),groups)
        self.assertEqual(result['ndcg10'],0.);self.assertEqual(result['returned'],0)

    def test_maximum_and_stable_identity_do_not_use_relevance(self):
        ids=np.array([9,4,4]);ordinals=np.array([20,12,8]);scores=np.array([.8,.7,.9])
        a=group(ids,ordinals,np.array([True,False,False]));b=group(ids,ordinals,np.array([False,True,False]))
        self.assertTrue(np.array_equal(a.blocks,b.blocks));self.assertTrue(np.array_equal(a.ordinals,b.ordinals))
        self.assertEqual(maximum(scores,a).tolist(),[.9,.8]);self.assertEqual(maximum(scores,b).tolist(),[.9,.8])

    def test_reject_duplicate_source_and_missing_score_ownership(self):
        with self.assertRaises(ValueError):group(np.array([0,0]),np.array([8,8]),np.array([True,False]))
        groups=group(np.array([0]),np.array([8]),np.array([True]))
        with self.assertRaises(ValueError):maximum(np.array([.5,.6]),groups)
        with self.assertRaises(ValueError):maximum(np.array([np.nan]),groups)


if __name__=='__main__':unittest.main()
