import unittest
from qmsum_discovery_controls import topic_support


class DiscoverySupportTests(unittest.TestCase):
    def test_near_pairs_and_unknowns_do_not_supply_rare_recurrence(self):
        self.assertEqual(topic_support([0,0,0],[0,0,None],[0,4,10],2),[
            {'topic':0,'rows':2,'far_relevant_pairs':0,'far_recovered_pairs':0,'far_recall':None},
            {'topic':1,'rows':0,'far_relevant_pairs':0,'far_recovered_pairs':0,'far_recall':None}])

    def test_original_gap_five_and_source_positive_pair_accounting(self):
        result=topic_support([0,1,0],[0,0,0],[0,5,10],1)[0]
        self.assertEqual((result['far_relevant_pairs'],result['far_recovered_pairs'],result['far_recall']),(3,1,1/3))
