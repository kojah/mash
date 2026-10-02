import random
import unittest
try:
    import numpy as np
    from episode_blocks import pool
    from validation_blocks import boundaries
except ImportError:
    boundaries = None


@unittest.skipIf(boundaries is None,'scientific NumPy/SciPy environment required')
class PhysicalBounds(unittest.TestCase):
    def test_reference_boundaries_match_without_geometry_and_including_cancellation(self):
        rng = random.Random(20261002)
        for _ in range(100):
            n = rng.randrange(1,200); ready = [rng.random() > .1 for i in range(n)]
            tokens = [rng.randrange(2,513) for i in range(n)]
            vectors = np.array([[1. if i%2 else -1.,0.] for i in range(n)])
            old,owners,_ = pool(vectors,np.array(ready),np.array(tokens))
            new,actual = boundaries(ready,tokens)
            self.assertEqual([(b.ordinal,b.start,b.end,b.tokens,b.close) for b in old],
                             [(b.ordinal,b.start,b.end,b.tokens,b.close) for b in new])
            self.assertEqual(tuple(owners),actual)

    def test_geometry_zero_does_not_drop_ready_original_source(self):
        blocks, owners = boundaries([True]*17+[False,True],[2]*19)
        self.assertEqual([(b.start,b.end) for b in blocks],[(0,15),(16,16),(18,18)])
        self.assertEqual(owners[17],-1)
        self.assertTrue(all(owners[i] >= 0 for i in range(19) if i != 17))
        self.assertEqual(boundaries([],[]),((),()))

    def test_invalid_readiness_and_tokens_fail_instead_of_truncating(self):
        for ready,tokens in [([1],[2]),([True],[513]),([False],[True]),([True],[]),([True],[1])]:
            with self.assertRaises(ValueError): boundaries(ready,tokens)


if __name__ == '__main__':unittest.main()
