import unittest
from soft_topic_guidance import retrieve


class Guidance(unittest.TestCase):
    def test_parent_bonus_reorders_without_excluding_direct_source(self):
        result=retrieve([.9,.8,0],[3,8,9],[10,11,10],[.1,.9,.5],[10,11,12])
        self.assertEqual([r['id'] for r in result['returned_sources']],[8,3])
        self.assertEqual(result['candidate_sources'],2)
        self.assertAlmostEqual(result['returned_sources'][0]['score'],1/61+1/62)
        self.assertNotIn(9,[r['id'] for r in result['returned_sources']])

    def test_no_parent_match_keeps_direct_order_and_zero_source_abstains(self):
        result=retrieve([.9,.8],[4,5],[8,9],[0,0],[8,9])
        self.assertEqual([r['id'] for r in result['returned_sources']],[4,5])
        self.assertTrue(all(r['parent_rank'] is None for r in result['returned_sources']))
        self.assertEqual(retrieve([0],[4],[8],[1],[8])['returned_sources'],[])

    def test_parent_outside_scope_rejected(self):
        with self.assertRaises(ValueError):retrieve([1],[3],[8],[1],[9])


if __name__=='__main__':unittest.main()
