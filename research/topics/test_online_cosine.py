import unittest

import numpy as np

from online_cosine import discover,diagnostics


class OnlineCosineTests(unittest.TestCase):
    def test_sum_updates_and_exact_tie_choose_first_created(self):
        x=np.array([[1.,0.],[0.,1.],[1.,1.]])/np.array([[1.],[1.],[np.sqrt(2)]])
        r=discover(x,threshold=.55)
        np.testing.assert_array_equal(r.assignments,[0,1,0])
        np.testing.assert_allclose(r.sums,[x[0]+x[2],x[1]],atol=0,rtol=0)
        self.assertEqual(r.comparisons,3)
        self.assertEqual(r.events[-1].nearest,0)

    def test_cutoff_equality_joins(self):
        r=discover(np.array([[1.,0.],[.5,np.sqrt(.75)]]),threshold=.5)
        self.assertEqual(len(r.counts),1)
        self.assertFalse(r.events[-1].created)

    def test_capacity_failure_keeps_unprocessed_and_does_not_mutate(self):
        r=discover(np.eye(3),maximum=1)
        self.assertFalse(r.complete)
        np.testing.assert_array_equal(r.assignments,[0,-1,-1])
        np.testing.assert_array_equal(r.sums,[[1,0,0]])
        self.assertIsNone(r.events[-1].assigned)
        self.assertEqual(diagnostics(np.eye(3),r)['unprocessed'],2)

    def test_reverse_uses_source_positions_not_arrival_positions(self):
        r=discover(np.eye(3),reverse=True)
        np.testing.assert_array_equal(r.assignments,[2,1,0])
        self.assertEqual([e.document for e in r.events],[2,1,0])

    def test_pair_cohesion_excludes_singletons_explicitly(self):
        x=np.array([[1.,0.],[1.,0.],[0.,1.]])
        d=diagnostics(x,discover(x))
        self.assertEqual(d['nonsingleton_topics'],1)
        self.assertAlmostEqual(d['macro_pair_cosine'],1)
        self.assertEqual(d['members_below_insertion_cutoff'],0)

    def test_invalid_vectors_and_budgets_rejected(self):
        for x in [np.zeros((1,2)),np.array([[np.nan,0]]),np.array([[2.,0.]])]:
            with self.assertRaises(ValueError):discover(x)
        for args in [{'maximum':0},{'threshold':0},{'reverse':1}]:
            with self.assertRaises(ValueError):discover(np.eye(2),**args)


if __name__=='__main__':unittest.main()
