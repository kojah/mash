import unittest
import math
from scipy import sparse
from stable_lexical_frame import State
from supported_labels import label,Profile,Support,prepare_background


class Labels(unittest.TestCase):
    def test_common_numeric_and_single_source_terms_excluded(self):
        terms=['123','database','just','pool','timeout']
        counts=sparse.csr_matrix([[2,2,1,2,3],[2,2,1,2,0]]+[[0,0,1,0,0]]*6)
        original=counts.copy();background=prepare_background(counts,terms);r=label([[0,1]],counts,terms,[State.READY]*8,background,Profile.DISTINCTIVE)[0]
        self.assertEqual({t['term'] for t in r.labels},{'database','pool'});self.assertEqual(r.support,Support.SUPPORTED)
        self.assertTrue(all(t['member_blocks']==2 for t in r.labels));self.assertEqual((counts-original).nnz,0)
        old=label([[0,1]],counts,terms,[State.READY]*8,background,Profile.STATIC)[0]
        self.assertIn('timeout',[t['term'] for t in old.labels])

    def test_fallback_abstains_without_losing_proposal_identity(self):
        c=sparse.csr_matrix([[2,2],[2,2],[0,0],[0,0],[0,0],[0,0]])
        background=prepare_background(c,['database','pool']);r=label([[0,1]],c,['database','pool'],[State.FALLBACK]*2+[State.READY]*4,background,Profile.DISTINCTIVE)[0]
        self.assertEqual(r.blocks,[0,1]);self.assertEqual(r.eligible_blocks,[]);self.assertEqual(r.labels,[]);self.assertEqual(r.support,Support.WEAK)
        with self.assertRaises(ValueError):label([[0,1],[1,2]],c,['database','pool'],[State.READY]*6,background,Profile.STATIC)

    def test_corpus_idf_orders_equally_frequent_class_terms(self):
        c=sparse.csr_matrix([[2,2],[2,2],[0,1]]+[[0,0]]*5)
        r=label([[0,1]],c,['database','pool'],[State.READY]*8,prepare_background(c,['database','pool']),Profile.DISTINCTIVE)[0]
        self.assertEqual(r.labels[0]['term'],'database');self.assertGreater(r.labels[0]['score'],r.labels[1]['score'])

    def test_static_scores_match_scalar_formula_across_unequal_classes(self):
        terms=['database','pool','timeout']
        c=sparse.csr_matrix([[4,2,0],[1,0,1],[0,1,6],[0,0,0]])
        result=label([[0,1],[2]],c,terms,[State.READY]*4,prepare_background(c,terms),Profile.STATIC)
        # Two class lengths 8 and 7; term frequencies 5, 3, 7.
        for proposal,counts in zip(result,[[5,2,1],[0,1,6]]):
            scores={t['term']:t['score'] for t in proposal.labels}
            for term,tf,frequency in zip(terms,counts,[5,3,7]):
                if tf:self.assertAlmostEqual(scores[term],tf/sum(counts)*math.log1p(7.5/frequency),places=14)


if __name__=='__main__':unittest.main()
