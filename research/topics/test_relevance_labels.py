import math
import unittest
from scipy import sparse
from relevance_labels import score,prepare,labels
from stable_lexical_frame import State


class Relevance(unittest.TestCase):
    def test_background_probability_can_reverse_equal_within_frequency(self):
        self.assertGreater(score(.1,.001),score(.1,.1))
        self.assertAlmostEqual(score(.1,.001),.6*math.log(.1)+.4*math.log(100),places=14)
        for p,q in [(0,.1),(.1,0),(float('nan'),.1),(1.1,.1)]:
            with self.assertRaises(ValueError):score(p,q)

    def test_source_support_and_signed_scores_preserved(self):
        terms=['123','database','pool','timeout'];c=sparse.csr_matrix([[1,2,2,5],[1,2,2,0]]+[[0,0,1,0]]*8)
        result=labels([[0,1]],c,terms,[State.READY]*10,prepare(c,terms))[0]
        self.assertEqual([t['term'] for t in result],['database'])
        # Numeric terms, one-block timeout and corpus-wide pool are ineligible.
        self.assertTrue(math.isfinite(result[0]['score']));self.assertEqual(result[0]['member_blocks'],2)
        self.assertEqual(labels([[0,1]],c,terms,[State.FALLBACK]*2+[State.READY]*8,prepare(c,terms)),[[]])

    def test_negative_relevance_is_not_silently_removed(self):
        terms=['database','pool'];c=sparse.csr_matrix([[1,1],[1,1]]+[[0,0]]*8)
        r=labels([[0,1]],c,terms,[State.READY]*10,prepare(c,terms))[0]
        self.assertEqual([t['term'] for t in r],terms);self.assertTrue(all(t['score']<0 for t in r))


if __name__=='__main__':unittest.main()
