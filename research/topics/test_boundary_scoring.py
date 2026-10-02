import unittest

import numpy as np

from boundary_scoring import calibrate,choose,confusion,score_turns


class BoundaryScoringTests(unittest.TestCase):
    def test_score_uses_only_past_and_current_and_never_crosses_dialogues(self):
        vectors=np.array([[1.,0.],[0.,1.],[-1.,0.],[0.,-1.]])
        scores,_=score_turns(vectors);short,_=score_turns(vectors[:3]);np.testing.assert_array_equal(scores[:2],short)
        self.assertEqual(scores[0,0],0.);self.assertEqual(scores[0,1],0.)
        self.assertAlmostEqual(scores[1,1],-1/np.sqrt(2))

    def test_canceling_context_abstains_without_changing_adjacent_support(self):
        scores,abstentions=score_turns([[1.,0.],[-1.,0.],[0.,1.]])
        self.assertEqual(scores[1,0],0.);self.assertTrue(np.isnan(scores[1,1]))
        self.assertEqual(abstentions,[{'ordinal':2,'method':'previous_two','reason':'canceling_context'}])

    def test_empty_zero_nonfinite_and_nonunit_inputs_rejected(self):
        for v in [[],[[1.,0.]],[[0.,0.],[1.,0.]],[[2.,0.],[1.,0.]],[[np.nan,0.],[1.,0.]]]:
            with self.assertRaises(ValueError):score_turns(v)

    def test_all_thresholds_match_independent_strict_prediction_counts(self):
        rng=np.random.default_rng(17)
        for _ in range(20):
            scores=rng.choice([-.5,0.,.25,.7],30);labels=rng.random(30)<.3
            result=calibrate(scores,labels)
            self.assertEqual(len(result['candidates']),len(set(scores))+1)
            for record in result['candidates']:self.assertEqual(record['counts'],confusion(labels,scores<record['threshold']))

    def test_equal_scores_never_split_and_all_none_endpoints_present(self):
        r=calibrate([.5,.5],np.array([True,False]));self.assertEqual(len(r['candidates']),2)
        self.assertEqual(r['candidates'][0]['counts']['tp'],0);self.assertEqual(r['winner']['counts']['tp'],1)
        self.assertEqual(r['winner']['counts']['fp'],1)

    def test_no_positive_tie_prefers_no_changes_and_method_tie_is_lexical(self):
        r=calibrate([-.5,0.,.5],np.array([False,False,False]));self.assertEqual(r['winner']['threshold'],-.5)
        self.assertEqual(choose({'previous_two':r,'adjacent':r}),'adjacent')

    def test_near_threshold_values_keep_exact_strict_order(self):
        x=.5;y=np.nextafter(x,np.inf);r=calibrate([x,y],np.array([True,False]))
        self.assertEqual(r['winner']['threshold'],y);self.assertEqual(r['winner']['counts']['f1'],1.)

    def test_bad_training_labels_and_unsupported_scores_are_not_silently_filtered(self):
        for scores,labels in [([.5],[1]),([np.nan],[True]),([2.],[True]),([] ,np.array([],dtype=bool))]:
            with self.assertRaises(ValueError):calibrate(scores,labels)


if __name__=='__main__':unittest.main()
