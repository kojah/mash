import unittest
import numpy as np

from compression_budget import graph,bound,Verdict


class CompressionBudgetTests(unittest.TestCase):
    def test_orthogonal_units_rule_out_half_retention(self):
        result=bound([graph(np.eye(20))])
        self.assertEqual(result['minimum_centroids_lower_bound'],19)
        self.assertEqual(result['maximum_groups_budget'],10)
        self.assertEqual(result['verdict'],Verdict.RULED_OUT)

    def test_shared_centroid_boundary_is_conservative(self):
        x=np.asarray([[.9,np.sqrt(1-.9**2)],[.9,-np.sqrt(1-.9**2)]])
        result=graph(x@x.T)
        self.assertEqual(result['neighbors'],[[1],[0]])
        self.assertEqual(result['isolated'],[])
        self.assertEqual(bound([result])['verdict'],Verdict.OPEN)

    def test_one_unit_and_global_outlier_budget_have_complete_support(self):
        single=graph(np.ones((1,1)));self.assertEqual(single['closest_cosine'],[None])
        result=bound([single,graph(np.eye(19))])
        self.assertEqual(result['maximum_outliers'],1);self.assertEqual(result['minimum_centroids_lower_bound'],19)

    def test_not_ruled_out_is_not_a_clustering_solution(self):
        result=graph(np.ones((4,4)))
        self.assertEqual(result['neighbors'],[[1,2,3],[0,2,3],[0,1,3],[0,1,2]])
        self.assertEqual(bound([result])['verdict'],Verdict.OPEN)

    def test_reject_invalid_geometry_and_duplicate_isolates(self):
        for gram in [np.zeros((2,2)),np.asarray([[1.,.2],[.3,1.]]),np.asarray([[1.,-1.,-1.],[-1.,1.,-1.],[-1.,-1.,1.]])]:
            with self.assertRaises(ValueError):graph(gram)
        with self.assertRaises(ValueError):bound([{'blocks':2,'isolated':[0,0]}])


if __name__=='__main__':unittest.main()
