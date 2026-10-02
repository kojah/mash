import unittest
import numpy as np
from query_navigation import Arm,query,rank,retrieve,metrics


class Navigation(unittest.TestCase):
    def test_zero_and_oov_queries_abstain_without_padding(self):
        for text in ['hello unknown', 'the and']:
            vector,metadata=query(text,{'database':0},np.ones(1))
            self.assertEqual(vector.nnz,0)
        self.assertEqual(metadata['known_terms'],0)
        self.assertEqual(rank([0.,0.],[1,2],20),[])

    def test_exact_ties_use_original_id_and_duplicate_id_rejected(self):
        self.assertEqual([r['id'] for r in rank([.5,.5,.6],[8,3,9],2)],[9,3])
        for scores,ids in [([1,1],[3,3]),([float('nan')],[1]),([1],[-1])]:
            with self.assertRaises(ValueError):rank(scores,ids,20)

    def test_navigation_is_fixed_gate_without_direct_fallback(self):
        args=([.9,.8,.7],[0,1,2],[10,11,12],[.4,0.,.8],[10,11,12])
        result=retrieve(*args,Arm.GROUP)
        self.assertEqual([r['id'] for r in result['selected_groups']],[12,10])
        self.assertEqual([r['id'] for r in result['returned_sources']],[0,2])
        empty=retrieve(*args[:3],[0,0,0],args[4],Arm.BLOCK)
        self.assertEqual(empty['returned_sources'],[])
        self.assertEqual(len(retrieve(*args,Arm.DIRECT)['returned_sources']),3)

    def test_unavailable_references_stay_in_denominators(self):
        result={'returned_sources':[{'id':1},{'id':0}]}
        m=metrics(result,[1,4],[0,1,2])
        self.assertEqual(m['recall_at_20'],.5);self.assertEqual(m['precision_at_20'],.05)
        self.assertEqual(m['candidate_recall'],.5);self.assertEqual(m['mrr'],1.)
        self.assertLess(m['ndcg_at_20'],1.)


if __name__=='__main__':unittest.main()
