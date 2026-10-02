from unittest import TestCase
import numpy as np
from incremental_nmf import mixture_shares,source_candidates,frame,snapshot
from topic_phrases import PhraseSource


class IncrementalNMFTests(TestCase):
    def test_scale_correction_and_zero_abstention(self):
        h=np.array([[2.,0],[0,1.]]);w=np.array([[.5,1.],[0,0]])
        shares=mixture_shares(w,h)
        np.testing.assert_array_equal(shares,np.array([[.5,.5],[0,0]]))
        np.testing.assert_array_equal(shares,mixture_shares(w/np.array([2.,4.]),h*np.array([2.,4.])[:,None]))
        self.assertEqual(source_candidates(shares),[[0,1],[]])
        for invalid in [np.array([[-1.,0]]),np.array([[float('nan'),0]])]:
            with self.assertRaises(ValueError):mixture_shares(invalid,h)

    def test_fixed_boundaries_and_ties(self):
        self.assertEqual(source_candidates(np.array([[.25,.25,.25,.25],[.6,.4,0,0],[.4,.3,.3,0]])),[[0,1,2],[0],[0]])
        # Binary64 .75*.4 exceeds literal .3; preserve exact kernel contract.
        with self.assertRaises(ValueError):source_candidates(np.array([[1.1]]))

    def test_frame_duplicate_votes_zero_rows_and_provider_scope(self):
        points=[PhraseSource('a','t','p','redis vector'),PhraseSource('b','t','p','redis vector'),PhraseSource('c','t','p','')]
        vocabulary,matrix,active,_=frame(points)
        self.assertEqual(active,[0]);self.assertEqual(matrix[2].nnz,0)
        other=frame([points[0],points[2]])
        self.assertEqual(vocabulary,other[0]);np.testing.assert_array_equal(matrix[0].toarray(),other[1][0].toarray())
        with self.assertRaises(ValueError):frame(points+[PhraseSource('d','t','other','query')])

    def test_snapshot_local_evidence_multitags_and_original_label_support(self):
        points=[PhraseSource('a','t','p','redis'),PhraseSource('b','u','p','postgres')]
        result=snapshot(points,['postgres','redis'],[{'redis':1},{'postgres':1}],np.eye(2),np.array([[0.,1.],[1.,0.]]),'fixture')
        self.assertEqual(result['diagnostics']['topics'],2)
        self.assertEqual({row['phrase'] for row in result['topics']},{'redis','postgres'})
        self.assertTrue(all(len(row['sources'])==1 for row in result['topics']))
