import unittest
from topic_pair_metrics import evaluate


class Pairs(unittest.TestCase):
    def test_abstention_preserves_recall_denominator(self):
        args=([0,1,2,3],[0,0,1,1],[0,1,0,1],[0,6,12,18])
        m=evaluate([[0,1]],*args);self.assertEqual(m['crossspan']['precision'],1);self.assertEqual(m['crossspan']['recall'],.5)
        empty=evaluate([],*args);self.assertIsNone(empty['all']['precision']);self.assertEqual(empty['all']['recall'],0);self.assertEqual(empty['all']['f1'],0)
        broad=evaluate([[0,1,2,3]],*args);self.assertAlmostEqual(broad['all']['precision'],1/3);self.assertEqual(broad['all']['recall'],1)

    def test_unknown_sources_coverage_and_same_block_pairs(self):
        m=evaluate([[0,1]],[0,0,1,2],[0,0,0,None],[0,0,1,None],[0,1,8,14])
        self.assertEqual(m['all']['gold_pairs'],2);self.assertEqual(m['all']['true_positive_pairs'],2)
        self.assertEqual(m['coverage']['ready_sources'],4);self.assertEqual(m['coverage']['assigned_sources'],3)
        self.assertEqual(m['coverage']['singleton_known_sources'],3)

    def test_recurring_does_not_call_same_span_pairs_negative(self):
        m=evaluate([[0,1,2]],[0,1,2],[0,0,1],[0,0,0],[0,6,12])
        self.assertEqual(m['crossspan']['gold_pairs'],0);self.assertEqual(m['crossspan']['predicted_pairs'],2)
        self.assertIsNone(m['crossspan']['recall']);self.assertEqual(m['crossspan']['precision'],0)


if __name__=='__main__':unittest.main()
