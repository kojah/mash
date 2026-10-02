import unittest
import numpy as np
from captured_hierarchy import Topology,temporal,Baseline
from semantic_cut import select


class SemanticCut(unittest.TestCase):
    def test_separates_coherent_pairs_without_gold_count(self):
        t=Topology(6,{4:(0,1),5:(2,3),6:(4,5)},{i:i for i in range(4)})
        v=np.array([[1,0],[1,0],[0,1],[0,1]],dtype=float);r=select(t,v)
        self.assertEqual(r.groups,[[0,1],[2,3]]);self.assertEqual(r.unassigned,[])
        self.assertAlmostEqual(r.mean,1/3);self.assertAlmostEqual(r.population_sd,np.std([1,0,0,0,0,1]))

    def test_duplicate_only_and_singletons_do_not_invent_topics(self):
        for n in [1,4]:
            r=select(temporal(n,Baseline.BALANCED),np.tile([1.,0.],(n,1)))
            self.assertEqual(r.groups,[]);self.assertEqual(r.unassigned,list(range(n)))
        v=np.array([1.,2.,3.]);v/=np.linalg.norm(v)
        self.assertEqual(select(temporal(11,Baseline.FORWARD),np.tile(v,(11,1))).groups,[])

    def test_child_orientation_and_source_vectors_preserved(self):
        t=temporal(9,Baseline.BALANCED);v=np.random.default_rng(20261002).normal(size=(9,4));v/=np.linalg.norm(v,axis=1)[:,None];before=v.copy()
        r=select(t,v);t.children.update({n:tuple(reversed(c)) for n,c in t.children.items()});other=select(t,v)
        self.assertEqual(r.groups,other.groups);self.assertEqual(r.unassigned,other.unassigned);np.testing.assert_array_equal(v,before)
        for node in r.nodes:
            g=node['blocks']
            if len(g)>1:
                direct=np.mean([v[a]@v[b] for i,a in enumerate(g) for b in g[i+1:]])
                self.assertAlmostEqual(node['distinct_pair_mean'],direct,places=12)
        for bad in [v*2,np.full((9,4),np.nan),v[:-1]]:
            with self.assertRaises(ValueError):select(t,bad)


if __name__=='__main__':unittest.main()
