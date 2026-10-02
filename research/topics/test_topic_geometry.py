import unittest

import numpy as np

from topic_geometry import bootstrap, diagnostics, directions
from topic_routing import Geometry, discover, memberships, posting_lists, route


class TopicGeometryTests(unittest.TestCase):
    def test_frozen_coordinates_chunk_equivalence_and_centering(self):
        x=np.random.default_rng(17).normal(size=(16,4));x/=np.linalg.norm(x,axis=1)[:,None]
        ids=[f'{i:02d}' for i in range(16)]
        p=bootstrap(x,ids,dimensions=2,size=8)
        before=p.components.copy()
        np.testing.assert_allclose(p.transform(x),np.vstack([p.transform(x[:7]),p.transform(x[7:])]),atol=1e-12)
        np.testing.assert_allclose(p.transform(x[list(p.bootstrap)]).mean(axis=0),0,atol=1e-12)
        np.testing.assert_allclose(p.components@p.components.T,np.eye(2),atol=1e-12)
        np.testing.assert_array_equal(before,p.components)
        with self.assertRaises(ValueError): p.components[0,0]=0

    def test_zero_projection_abstains_without_dropping_sources(self):
        raw=np.array([[0.,0.],[1.,0.]])
        normalized,valid,lengths=directions(raw)
        np.testing.assert_array_equal(valid,[False,True])
        centers=np.eye(2)
        groups=memberships(normalized,centers,normalized,np.array([0,1]))
        self.assertEqual(groups['hard-balanced'][0],())
        self.assertEqual(groups['overlap-max-window'][0],())
        posts=posting_lists(groups['hard-balanced'],2)
        self.assertEqual(route(normalized[0],centers,posts)[1],[])
        report=diagnostics(np.array([[1.,0.],[1.,0.],[0.,1.]]),np.zeros((3,2)),np.zeros((3,2)),[0,1,2])
        self.assertEqual(report['source_pairs'],3)
        self.assertEqual(report['zero_source_distance_pairs'],1)
        self.assertIsNone(report['absolute_cosine_change_p95'])

    def test_raw_radius_geometry_is_not_silently_normalized(self):
        raw=np.array([[.2,0.],[0.,.2]])
        means,unit,ids,trace=discover(raw,geometry=Geometry.PCA)
        self.assertEqual(len(means),1)
        np.testing.assert_allclose(means,[[.1,.1]],atol=1e-12)
        with self.assertRaises(ValueError): discover(raw)


if __name__=='__main__': unittest.main()
