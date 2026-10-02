import unittest
try:
    import numpy as np
    from validation_bootstrap import paired
except ImportError:
    paired = None


@unittest.skipIf(paired is None,'scientific NumPy environment required')
class MeetingBootstrap(unittest.TestCase):
    def test_unequal_meeting_query_counts_against_independent_expanded_oracle(self):
        ids = [0,1,1,2,2,2];deltas = [.2,.1,-.3,.4,.5,-.6]
        result = paired(ids,deltas,3)
        draws = np.random.Generator(np.random.PCG64(20261002)).integers(0,3,size=(10000,3))
        groups = [[v for i,v in zip(ids,deltas) if i == m] for m in range(3)]
        values = np.array([np.mean([v for m in draw for v in groups[m]]) for draw in draws])
        bounds = np.percentile(values,[2.5,97.5])
        self.assertAlmostEqual(result['point'],sum(deltas)/len(deltas),places=14)
        self.assertAlmostEqual(result['lower95'],bounds[0],places=14)
        self.assertAlmostEqual(result['upper95'],bounds[1],places=14)
        self.assertEqual(result,paired(ids,deltas,3))

    def test_zero_paired_effect_stays_zero_and_missing_meetings_fail(self):
        result = paired([0,1,1],[0.,0.,0.],2)
        self.assertEqual((result['point'],result['lower95'],result['upper95']),(0.,0.,0.))
        with self.assertRaises(ValueError):paired([0],[.1],2)
        with self.assertRaises(ValueError):paired([0],[float('nan')],1)
        with self.assertRaises(ValueError):paired([-1],[.1],1)


if __name__ == '__main__':unittest.main()
