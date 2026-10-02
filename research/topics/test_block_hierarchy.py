import unittest
import json
import numpy as np

from block_hierarchy import average,evaluate
from captured_hierarchy import Baseline,temporal
from qmsum_block_hierarchy import stored_tree


class BlockHierarchyTests(unittest.TestCase):
    def test_serialized_node_ids_survive_strict_json_repeat(self):
        tree=stored_tree(temporal(12,Baseline.BALANCED))
        self.assertEqual(json.dumps(tree,sort_keys=True),json.dumps(json.loads(json.dumps(tree)),sort_keys=True))

    def test_mixed_blocks_keep_source_fractions_and_exclude_internal_pairs(self):
        result=evaluate(temporal(2,Baseline.BALANCED),[0,0,0,1,1],[0,0,1,0,1],[0,0,0,1,1],[0,1,2,10,11],2)
        for scope in result.values():
            self.assertEqual(scope['pairs'],3)
            self.assertAlmostEqual(scope['purity'],(2*3/5+2/5)/3)
            self.assertEqual([r['pairs'] for r in scope['topics']],[2,1])

    def test_near_and_same_native_span_accounting_is_separate(self):
        result=evaluate(temporal(3,Baseline.BALANCED),[0,1,2],[0,0,0],[0,1,1],[0,3,10],1)
        self.assertEqual([result[s]['pairs'] for s in ['all','far','crossspan']],[3,2,1])
        self.assertEqual(result['crossspan']['purity'],1.)

    def test_unknown_block_and_zero_vectors_are_retained(self):
        tree,merges=average(np.eye(3))
        result=evaluate(tree,[0,1,2],[0,None,0],[0,None,1],[0,5,10],2)
        self.assertEqual(len(merges),2);self.assertEqual(result['all']['pairs'],1)
        self.assertEqual(result['all']['purity'],1.);self.assertIsNone(result['all']['topics'][1]['purity'])
        single,_=average(np.zeros((1,1)))
        self.assertIsNone(evaluate(single,[0,0],[0,0],[0,1],[0,10],1)['crossspan']['purity'])

    def test_average_links_semantic_pairs_and_ties_repeat(self):
        x=np.asarray([[1.,0.],[0.,1.],[1.,0.],[0.,1.]])
        first,merges=average(x@x.T);second,again=average(x@x.T)
        self.assertEqual(merges,again);self.assertEqual(first,second)
        self.assertEqual(evaluate(first,list(range(4)),[0,1,0,1],[0,0,1,1],[0,8,16,24],2)['all']['purity'],1.)

    def test_reject_incomplete_owner_alignment_and_asymmetric_scores(self):
        with self.assertRaises(ValueError):average(np.asarray([[1.,.2],[.3,1.]]))
        with self.assertRaises(ValueError):evaluate(temporal(2,Baseline.BALANCED),[0],[0],[0],[0],1)


if __name__=='__main__':unittest.main()
