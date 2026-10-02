import unittest

from links_thresholds import Parameters


class LinksThresholdTests(unittest.TestCase):
    def test_singletons_and_size_dependence(self):
        p=Parameters()
        self.assertEqual(p.pair(1,1),.25)
        values=[p.pair(k) for k in [1,2,10,100,10000]]
        self.assertEqual(values,sorted(values))
        self.assertLess(values[-1],.5)
        self.assertAlmostEqual(p.pair(10**12),.5,places=10)

    def test_pair_symmetry_and_isotropic_limit(self):
        p=Parameters(tc=.6,ts=.85,tp=1)
        self.assertEqual(p.pair(3,11),p.pair(11,3))
        self.assertAlmostEqual(p.pair(10**12,10**12),1,places=10)

    def test_anisotropic_limits_and_degenerate_boundary(self):
        p=Parameters(tc=.5,ts=.8,tp=.9)
        self.assertEqual(p.pair(1,1),.25)
        self.assertAlmostEqual(p.pair(10**12,10**12),.9,places=10)
        self.assertEqual(Parameters(tp=.25).pair(10,100),.25)

    def test_invalid_domains_and_counts_rejected(self):
        for args in [{'tc':0},{'tc':1},{'ts':float('nan')},{'tp':.2},{'tp':2}]:
            with self.assertRaises(ValueError):Parameters(**args)
        for k in [0,-1,1.,True]:
            with self.assertRaises(ValueError):Parameters().pair(k)


if __name__=='__main__':unittest.main()
