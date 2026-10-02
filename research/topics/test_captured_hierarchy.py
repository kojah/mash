import unittest

from captured_hierarchy import Baseline,Topology,evaluate,temporal,traversal
from qmsum_hierarchy import compare_captures


class CapturedHierarchyTests(unittest.TestCase):
    def test_repeat_compares_every_field_without_timing_envelope(self):
        capture={'scope':'diagnostic','pairs':4,'metrics':[None,.5],'code_pin':'a'}
        compare_captures(capture,dict(capture))
        for change in [{'pairs':4.},{'metrics':[None,.6]},{'code_pin':'b'},{'timings':{}}]:
            with self.assertRaises(ValueError):compare_captures(capture,{**capture,**change})

    def test_known_hierarchy_and_return_components(self):
        tree = Topology(6,{4:(0,2),5:(1,3),6:(4,5)},{i:i for i in range(4)})
        gold = [0,1,0,1];components = [0,0,1,1];ordinals = [0,1,8,9]
        perfect = evaluate(tree,gold,components,ordinals,2)
        for scope in perfect:
            self.assertEqual(perfect[scope]['pairs'],2);self.assertEqual(perfect[scope]['purity'],1.)
            self.assertEqual(perfect[scope]['root_only_purity'],.5)
        baseline = evaluate(temporal(4,Baseline.BALANCED),gold,components,ordinals,2)
        self.assertEqual(baseline['crossspan']['purity'],.5)
        self.assertEqual(baseline['crossspan']['root_lca_pairs'],2)

    def test_unknowns_and_overlapping_context_are_explicit(self):
        tree = temporal(4,Baseline.BALANCED)
        result = evaluate(tree,[0,None,0,1],[0,None,1,0],[0,1,3,4],2)
        self.assertEqual(result['all']['pairs'],1);self.assertAlmostEqual(result['all']['purity'],2/3)
        self.assertEqual(result['far']['pairs'],0);self.assertIsNone(result['far']['purity'])
        self.assertEqual(result['crossspan']['pairs'],0)
        no_returns = evaluate(tree,[0,0,0,0],[0,0,0,0],[0,8,16,24],1)
        self.assertEqual(no_returns['far']['purity'],1.)
        self.assertIsNone(no_returns['crossspan']['purity'])

    def test_reject_invalid_ownership_and_connectivity(self):
        for tree in [Topology(2,{2:(0,1)},{0:0,1:0}),Topology(2,{2:(0,0)},{0:0,1:1}),
                     Topology(2,{2:(0,1),3:(0,1)},{0:0,1:1}),Topology(2,{2:(0,2)},{0:0,1:1})]:
            with self.assertRaises(ValueError):traversal(tree,2)

    def test_combs_own_original_positions_and_have_expected_purity(self):
        for method in Baseline:
            tree = temporal(4,method);traversal(tree,4)
            result = evaluate(tree,[0,0,1,1],[0,0,0,0],[0,8,16,24],2)
            expected = 1. if method == Baseline.BALANCED else .75
            self.assertEqual(result['all']['purity'],expected)


if __name__ == '__main__':unittest.main()
