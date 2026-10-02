import unittest
from captured_hierarchy import temporal,Baseline
from calibration_metrics import post_bootstrap,compact
from block_hierarchy import evaluate


class Bootstrap(unittest.TestCase):
    def test_old_sources_remain_in_lca_denominator(self):
        t=temporal(4,Baseline.BALANCED);owners=[0,1,2,3];gold=[1,1,0,0];parts=[0,0,0,1];ordinals=[0,6,12,18]
        m=post_bootstrap(t,owners,gold,parts,ordinals,2)
        self.assertEqual(m['crossspan']['pairs'],1);self.assertEqual(m['crossspan']['purity'],1)
        # The streaming pair's LCA includes a bootstrap negative at leaf0.
        t=temporal(3,Baseline.FORWARD);m=post_bootstrap(t,[0,1,2],[1,0,0],[0,0,1],[0,6,12],1)
        self.assertAlmostEqual(m['crossspan']['purity'],2/3)

    def test_zero_bootstrap_matches_original_full_metrics_and_empty_support(self):
        t=temporal(4,Baseline.REVERSE);args=([0,0,1,2,3],[0,0,None,0,1],[0,0,None,1,0],[0,1,6,12,18])
        self.assertEqual(post_bootstrap(t,*args,0),compact(evaluate(t,*args,2)))
        self.assertIsNone(post_bootstrap(t,*args,4)['all']['purity'])


if __name__=='__main__':unittest.main()
