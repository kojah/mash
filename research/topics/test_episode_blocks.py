import unittest

import numpy as np
from scipy import sparse

from episode_blocks import End,block_counts,lexical,pool
from return_ranking import evaluate,fusion,rank


class EpisodeBlockTests(unittest.TestCase):
    def test_boundaries_preserve_gaps_tokens_and_immutable_sources(self):
        vectors = np.tile([1.,0.],(20,1));vectors.flags.writeable = False
        ready = np.ones(20,dtype=bool);ready[17] = False;tokens = np.full(20,2)
        blocks,owner,directions = pool(vectors,ready,tokens)
        self.assertEqual([(b.start,b.end,b.close) for b in blocks],[(0,15,End.TURNS),(16,16,End.GAP),(18,19,End.MEETING)])
        self.assertEqual(owner[17],-1);self.assertTrue(np.array_equal(vectors,np.tile([1.,0.],(20,1))))
        blocks,_,_ = pool(vectors[:3],ready[:3],np.array([400,100,20]))
        self.assertEqual([(b.start,b.end,b.tokens,b.close) for b in blocks],[(0,1,500,End.TOKENS),(2,2,20,End.MEETING)])

    def test_cancellation_and_lexical_counts_do_not_duplicate_sources(self):
        blocks,owner,directions = pool(np.array([[1.,0.],[-1.,0.]]),np.ones(2,dtype=bool),np.array([2,2]))
        self.assertTrue(blocks[0].cancelled);self.assertFalse(directions.any())
        counts = sparse.csr_matrix([[2,0],[1,4]]);combined = block_counts(counts,owner,1)
        self.assertEqual(combined.toarray().tolist(),[[3.,4.]])
        weighted = lexical(combined,np.array([2.,1.])).toarray()[0]
        expected = np.array([(1+np.log(3))*2,1+np.log(4)]);expected /= np.linalg.norm(expected)
        np.testing.assert_allclose(weighted,expected,rtol=0,atol=1e-15)

    def test_positive_only_ranking_ties_and_missing_relevant_results(self):
        scores = np.array([.5,.5,0.,-.1]);ordinals = np.array([8,4,12,16]);relevant = np.array([False,True,True,False])
        self.assertEqual(rank(scores,ordinals).tolist(),[1,0])
        result = evaluate(scores,ordinals,relevant)
        self.assertEqual(result['relevant_ranks'],[1]);self.assertEqual(result['ap'],.5)
        self.assertEqual(result['rr'],1.);self.assertEqual(result['returned'],2)
        abstain = evaluate(np.zeros(4),ordinals,relevant)
        self.assertEqual(abstain['ap'],0.);self.assertFalse(abstain['hit10'])
        fused = fusion(scores,np.array([0.,0.,1.,0.]),ordinals)
        self.assertAlmostEqual(fused[2],1/61);self.assertEqual(fused[3],0.)

    def test_reject_invalid_directions_support_and_counts(self):
        with self.assertRaises(ValueError):pool(np.array([[0.,0.]]),np.array([True]),np.array([2]))
        with self.assertRaises(ValueError):pool(np.array([[1.,0.]]),np.array([True]),np.array([513]))
        with self.assertRaises(ValueError):lexical(sparse.csr_matrix([[-1.]]),np.array([1.]))


if __name__=='__main__':unittest.main()
