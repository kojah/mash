import os
import random
import unittest

import numpy as np

from fishdbc_reference import exact,matrix,run,topology


class GraphContract(unittest.TestCase):
    def test_other_neighbor_and_tied_kruskal(self):
        d=np.ones((6,6));np.fill_diagonal(d,0)
        cores,edges=exact(d)
        self.assertEqual(cores,[1.]*6)
        self.assertEqual([(e[1],e[2]) for e in edges],[(0,i) for i in range(1,6)])

    def test_source_mapping_and_bad_merges(self):
        merges=[[0,1,1,2],[2,3,2,3],[4,5,3,4],[6,7,4,5],[8,9,5,6]]
        tree=topology(merges,list(range(5,-1,-1)))
        self.assertEqual(tree.leaves[0],5)
        with self.assertRaises(ValueError):topology(merges,[0]*6)
        merges[0][0]=6
        with self.assertRaises(ValueError):topology(merges,list(range(6)))

    def test_reject_bad_distance(self):
        for d in [np.ones((5,5)),np.ones((6,6)),np.full((6,6),np.nan),np.eye(6),np.zeros((6,5))]:
            with self.assertRaises(ValueError):matrix(d)

    @unittest.skipUnless(os.environ.get('MASH_TOPIC_FISHDBC_REFERENCE'),'explicit compiled author reference required')
    def test_author_prefixes_repeat_and_random_isolation(self):
        points=np.arange(34,dtype=np.float64);d=np.abs(points[:,None]-points[None,:])
        state=random.getstate();a,_=run(d,os.environ['MASH_TOPIC_FISHDBC_REFERENCE']);b,_=run(d,os.environ['MASH_TOPIC_FISHDBC_REFERENCE'])
        self.assertEqual(random.getstate(),state);self.assertEqual(a,b)
        self.assertEqual([p['size'] for p in a['prefixes']],[16,32,34])
        self.assertGreaterEqual(a['mst_weight_excess'],-1e-12)
        # The author's pending reachability updates can overwrite the field
        # commented as raw distance. Preserve it, never interpret it as raw.
        self.assertTrue(any(e[3]>d[a['order'][e[1]],a['order'][e[2]]]+1e-12 for e in a['native_edges']))
        for p in a['prefixes']:topology(p['merges'],p['source_order'])


if __name__=='__main__':unittest.main()
