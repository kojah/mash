import unittest
import numpy as np

from compressed_macro import compress,macro
from captured_hierarchy import traversal
from block_hierarchy import evaluate


class CompressedMacroTests(unittest.TestCase):
    def test_snapshot_owns_duplicates_in_both_orders(self):
        vectors=np.asarray([[1.,0.],[0.,1.],[1.,0.],[0.,1.]])
        for reverse in [False,True]:
            result=compress(vectors,reverse)
            self.assertEqual(result.groups,[[0,2],[1,3]])
            self.assertEqual(sum(c['samples'] for c in result.state['physical_subclusters']),4)
            tree,merges=macro(vectors,result.groups);traversal(tree,4)
            self.assertEqual(merges[0]['distance'],1.)
            self.assertEqual(evaluate(tree,list(range(4)),[0,1,0,1],[0,0,1,1],[0,8,16,24],2)['all']['purity'],1.)

    def test_mean_directions_are_not_renormalized(self):
        vectors=np.asarray([[1.,0.],[0.,1.],[1.,0.]])
        _,merges=macro(vectors,[[0,1],[2]])
        self.assertEqual(merges[0]['distance'],.5)

    def test_populations_weight_macro_updates(self):
        vectors=np.asarray([[1.,0.],[1.,0.],[.8,.6],[0.,1.]])
        _,merges=macro(vectors,[[0,1],[2],[3]])
        self.assertAlmostEqual(merges[0]['distance'],.2)
        self.assertAlmostEqual(merges[1]['distance'],.8)
        self.assertEqual(merges[1]['blocks'],4)

    def test_empty_macro_and_ties_have_deterministic_complete_lifting(self):
        tree,merges=macro(np.eye(3),[[0,1,2]])
        self.assertEqual(merges,[]);traversal(tree,3)
        a,first=macro(np.eye(4),[[0],[1],[2],[3]])
        b,second=macro(np.eye(4),[[0],[1],[2],[3]])
        self.assertEqual((a,first),(b,second));self.assertEqual(first[0]['children'],[0,1])

    def test_reject_invalid_unit_input_and_ownership(self):
        with self.assertRaises(ValueError):compress(np.zeros((1,2)))
        with self.assertRaises(ValueError):macro(np.eye(2),[[0],[0]])


if __name__=='__main__':unittest.main()
