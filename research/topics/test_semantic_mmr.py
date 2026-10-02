import unittest

import numpy as np

from semantic_mmr import select


class MMRTests(unittest.TestCase):
    def test_relevance_redundancy_and_lexical_ties(self):
        vectors=np.array([[1.,0.],[1.,0.],[0.,1.]])
        steps=select(['a','b','c'],[1.,.95,.8],vectors,limit=2)
        self.assertEqual([s['term'] for s in steps],['a','c'])
        self.assertEqual(steps[0]['maximum_redundancy'],0)
        self.assertAlmostEqual(steps[1]['objective'],.48)
        self.assertEqual([s['term'] for s in select(['b','a'],[1,1],np.eye(2))],['a','b'])

    def test_signed_cosines_and_invalid_pools(self):
        steps=select(['a','b'],[1,.5],np.array([[1.,0.],[-1.,0.]]))
        self.assertEqual(steps[1]['maximum_redundancy'],-1)
        self.assertAlmostEqual(steps[1]['objective'],.7)
        for terms,scores in [(['a','a'],[1,1]),(['a','b'],[0,1]),(['a'],[float('nan')])]:
            with self.assertRaises(ValueError): select(terms,scores,np.eye(2)[:len(terms)])


if __name__=='__main__': unittest.main()
