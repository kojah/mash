import unittest
import numpy as np
from soft_topic_guidance import retrieve as reference
from vector_topic_guidance import retrieve


class ExactVector(unittest.TestCase):
    def test_empty_source_and_group_inventory(self):
        integers=np.array([],dtype=np.int64);scores=np.array([],dtype=float)
        args=(scores,integers,integers,scores,integers)
        self.assertEqual(retrieve(*args),reference(*args))
        self.assertEqual(retrieve(*args)['returned_sources'],[])

    def test_generated_permutations_ties_zero_and_negative_match_every_field(self):
        rng=np.random.default_rng(104203)
        for _ in range(200):
            n=int(rng.integers(0,300));ng=int(rng.integers(1,25))
            gids=rng.permutation(np.arange(ng,dtype=np.int64)*1009+7)
            ids=rng.permutation(np.arange(n,dtype=np.int64)*997+13)
            owners=rng.choice(gids,size=n);sources=rng.choice([-.5,0.,.1,.2,.9],size=n);groups=rng.choice([-.1,0.,.2,.8],size=ng)
            self.assertEqual(retrieve(sources,ids,owners,groups,gids),reference(sources,ids,owners,groups,gids))

    def test_near_threshold_float_values_are_not_rounded_or_clipped(self):
        ids=np.arange(100,dtype=np.int64);gids=np.arange(4,dtype=np.int64)
        source=np.full(100,.5);source[2]=np.nextafter(.5,1.);source[8]=np.nextafter(.5,0.)
        groups=np.array([.7,np.nextafter(.7,1.),np.nextafter(.7,0.),0.]);owners=ids%4
        self.assertEqual(retrieve(source,ids,owners,groups,gids),reference(source,ids,owners,groups,gids))

    def test_invalid_or_missing_identity_rejected(self):
        for args in [([1,1],[3,3],[8,8],[1],[8]),([float('nan')],[3],[8],[1],[8]),([1],[3],[9],[1],[8]),([1],[3],[8.5],[1],[8])]:
            with self.assertRaises(ValueError):retrieve(*args)


if __name__=='__main__':unittest.main()
