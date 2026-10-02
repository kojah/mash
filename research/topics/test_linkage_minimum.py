import unittest
import numpy as np
from linkage_minimum import verify


class Minimum(unittest.TestCase):
    def test_valid_unequal_population_average(self):
        d=np.array([[0,.3,.5],[.3,0,.1],[.5,.1,0]])
        self.assertEqual(verify(d,[[1,2,.1,2],[0,3,.4,3]])['merges'],2)
        with self.assertRaises(ValueError):verify(d,[[0,1,.3,2],[2,3,.3,3]])

    def test_ties_allowed_but_inactive_children_and_bad_counts_rejected(self):
        d=np.ones((3,3));np.fill_diagonal(d,0)
        self.assertEqual(verify(d,[[0,2,1,2],[1,3,1,3]])['maximum_chosen_minus_global_minimum'],0)
        with self.assertRaises(ValueError):verify(d,[[0,2,1,2],[0,1,1,3]])
        with self.assertRaises(ValueError):verify(d,[[0,2,1,3],[1,3,1,3]])


if __name__=='__main__':unittest.main()
