import unittest
from qmsum_semantic_discovery import nonoverlap


class NonoverlapTests(unittest.TestCase):
    def test_exclusions_use_original_ordinals_and_retain_unknown_support(self):
        r=nonoverlap([0,0,0,1],[0,0,None,1],[0,4,5,10])
        self.assertEqual(r,{'known_rows':3,'pairs':2,'tp':0,'fp':0,'fn':0,'tn':2})

    def test_distance_five_is_retained_and_negative_and_positive_pairs_accounted(self):
        r=nonoverlap([0,0,1],[0,1,0],[0,5,10])
        self.assertEqual(r,{'known_rows':3,'pairs':3,'tp':0,'fp':1,'fn':1,'tn':1})
        with self.assertRaises(ValueError):nonoverlap([0,0],[0,0],[5,1])
