import unittest

import numpy as np

from topic_filtered_retrieval import admitted_union, score_union
from topic_routing import Match


class FilteredRetrievalTests(unittest.TestCase):
    def test_union_deduplication_work_limit_and_no_fallback(self):
        posts = [[(0,.8),(1,.7)],[(1,.9),(2,.6)],[(3,1.)]]
        selected = [Match(0,.9),Match(1,.8)]
        self.assertEqual(admitted_union(selected,posts,4),([0,1,2],4))
        self.assertEqual(admitted_union([],posts,4),([],0))
        with self.assertRaises(ValueError): admitted_union(selected,posts,4,maximum=2)
        with self.assertRaises(ValueError): admitted_union([Match(0,.9)], [[(1,.7),(0,.8)]],2)

    def test_signed_relevance_ties_and_chunk_boundaries(self):
        x=np.tile([0.,1.],(520,1));x[0]=[1,0];x[519]=[-1,0]
        scores,selected=score_union(np.array([1.,0.]),x,list(range(520)),budget=3)
        np.testing.assert_array_equal(scores,np.sum(x*np.array([1.,0.]),axis=1))
        self.assertEqual(selected,[(0,1.),(1,0.),(2,0.)])
        self.assertEqual(score_union(np.array([1.,0.]),x,[])[1],[])
        # An excellent outside-union source never enters the result.
        self.assertEqual(score_union(np.array([1.,0.]),x,[519])[1],[(519,-1.)])
        for union in [[2,1],[1,1],[-1],[520]]:
            with self.assertRaises(ValueError): score_union(np.array([1.,0.]),x,union)


if __name__=='__main__': unittest.main()
