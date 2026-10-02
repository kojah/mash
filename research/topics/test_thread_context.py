import unittest
import numpy as np
from query_navigation import rank
from vector_topic_guidance import retrieve


class OneThread(unittest.TestCase):
    def test_shared_parent_bonus_preserves_source_order_including_ties_and_ulp(self):
        scores=np.array([.4,np.nextafter(.4,1.),.4,0.,-.2,.9]);ids=np.array([99,33,10,9,4,8],dtype=np.int64)
        for group_score in [0.,.1,1.]:
            result=retrieve(scores,ids,np.full(len(ids),17,dtype=np.int64),[group_score],[17])
            self.assertEqual([r['id'] for r in result['returned_sources']],[r['id'] for r in rank(scores,ids,20)])

    def test_complete_budget_and_zero_queries_preserve_direct_counts(self):
        for scores in [np.linspace(.01,.8,1000),np.zeros(1000)]:
            ids=np.arange(len(scores),dtype=np.int64);result=retrieve(scores,ids,np.ones(len(ids),dtype=np.int64),[.5],[1])
            self.assertEqual([r['id'] for r in result['returned_sources']],[r['id'] for r in rank(scores,ids,20)])


if __name__=='__main__':unittest.main()
