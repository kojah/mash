import unittest

import numpy as np

from topic_routing import DiscoveryBudget, Match, discover, memberships, merge_top, posting_lists, route, routed_metrics, select


class TopicRoutingTests(unittest.TestCase):
    def test_threshold_ties_budget_and_noise(self):
        self.assertEqual(select([.5499999, .1]), ())
        self.assertEqual(select([.55, .55, .55, .55]), tuple(Match(i, .55) for i in range(3)))
        self.assertEqual(select([1, .85, .849999]), (Match(0,1), Match(1,.85)))
        for scores in [[], [np.nan], [2]]:
            with self.assertRaises(ValueError): select(scores)

    def test_max_window_reduction_preserves_top_three(self):
        rng = np.random.default_rng(17)
        for _ in range(30):
            values = rng.integers(0,10,size=(5,9))/10
            reduced = ()
            for row in values:
                chosen = np.argsort(-row,kind='stable')[:3]
                reduced = merge_top(reduced, tuple(Match(int(i),float(row[i])) for i in chosen))
            full = values.max(axis=0)
            expected = tuple(Match(int(i),float(full[i])) for i in np.argsort(-full,kind='stable')[:3])
            self.assertEqual(reduced, expected)

    def test_routing_uses_membership_products_and_no_dense_fallback(self):
        centers = np.eye(2)
        posts = posting_lists([(Match(0,.6), Match(1,.8)), (Match(0,.9),), (Match(0,.9),)],2)
        topics, chosen, union, visits = route(np.array([1.,0]),centers,posts,budget=2)
        self.assertEqual(chosen,[(1,.9),(2,.9)])
        self.assertEqual((union,visits),(3,3))
        self.assertEqual(route(np.array([-1.,0]),centers,posts)[1],[])
        record = routed_metrics([],[],['a','b'],{'b'})
        self.assertEqual(record['candidate_recall'],0)
        self.assertEqual(record['relevant_ranks'],{'b':None})
        self.assertTrue(all(x==0 for x in record['metrics'].values()))

    def test_complete_memberships_and_insert_only_discovery(self):
        x = np.array([[1.,0],[0,1]])
        raw, centers, ids, trace = discover(x)
        self.assertEqual(trace[-1]['inserted'],2)
        self.assertEqual(ids,sorted(ids))
        members = memberships(x,centers,x,np.array([0,1]))
        self.assertEqual(len(members['hard-balanced']),2)
        with self.assertRaises(ValueError): memberships(x,centers,x,np.array([0,0]))
        with self.assertRaises(DiscoveryBudget) as failure: discover(x,maximum=1)
        self.assertEqual(failure.exception.maximum,1)
        self.assertEqual(failure.exception.trace,[{'inserted':2,'centroids':2}])


if __name__ == '__main__': unittest.main()
