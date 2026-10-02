import unittest
from captured_hierarchy import Topology,temporal,Baseline
from matched_unit_hierarchy import evaluate


class Matched(unittest.TestCase):
    def test_fine_and_coarse_trees_have_identical_positive_support(self):
        fine=temporal(4,Baseline.BALANCED);coarse=temporal(2,Baseline.BALANCED)
        args=([0,0,1,1],[0,0,0,1],[0,0,1,0],[0,1,8,9])
        a=evaluate(fine,[0,1,2,3],*args);b=evaluate(coarse,[0,0,1,1],*args)
        self.assertEqual(a['all']['pairs'],2);self.assertEqual(b['all']['pairs'],2)
        self.assertEqual(a['crossspan']['pairs'],2);self.assertEqual(b['crossspan']['pairs'],2)
        self.assertEqual(a['all']['purity'],.75);self.assertEqual(b['all']['purity'],.75)

    def test_unknown_geometry_and_empty_positive_support(self):
        t=temporal(3,Baseline.BALANCED)
        m=evaluate(t,[0,1,2],[0,1,2],[0,None,0],[0,None,1],[0,4,10])
        self.assertEqual(m['all']['purity'],1);self.assertEqual(m['far']['pairs'],1)
        empty=evaluate(t,[0,1,2],[0,1,2],[None]*3,[None]*3,[0,4,10])
        self.assertIsNone(empty['all']['purity']);self.assertEqual(empty['all']['pairs'],0)

    def test_coarsening_across_exclusion_units_is_rejected(self):
        with self.assertRaises(ValueError):evaluate(temporal(1,Baseline.BALANCED),[0,0],[0,1],[0,0],[0,1],[0,8])


if __name__=='__main__':unittest.main()
