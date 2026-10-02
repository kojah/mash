import unittest

import numpy as np

from topic_prototypes import complete_union, fit
from topic_routing import Match, select


class PrototypeTests(unittest.TestCase):
    def test_equal_document_mass_and_inactive_topics(self):
        vectors=np.array([[1.,0.],[-1.,0.],[0.,1.]])
        p=fit(vectors,[(Match(0,.8),Match(1,.7)),(Match(0,.6),Match(1,.9)),(Match(2,.9),)],4)
        np.testing.assert_array_equal(p.masses,[1,1,1,0])
        np.testing.assert_array_equal(p.sums,[[0,0],[0,0],[0,1],[0,0]])
        np.testing.assert_array_equal(p.active,[False,False,True,False])
        scores=p.directions@np.array([1.,0.]);scores[~p.active]=-1
        self.assertEqual(select(scores),())
        with self.assertRaises(ValueError): fit(vectors,[(Match(0,.8),)]*2,1)

    def test_complete_union_deduplicates_and_keeps_over_budget_metadata(self):
        row=(Match(0,.8),Match(1,.9))
        members=[row]*8193+[()]
        union,visits=complete_union([Match(0,.8),Match(1,.9)],members)
        self.assertEqual(union,list(range(8193)))
        self.assertEqual(visits,16386)
        self.assertEqual(complete_union([],members),([],0))


if __name__=='__main__': unittest.main()
