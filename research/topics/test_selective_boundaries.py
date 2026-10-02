import unittest

import numpy as np

from selective_boundaries import evaluate, reliability, select


class SelectiveBoundaryTests(unittest.TestCase):
    def test_unsupported_never_falls_back_or_claims_no_change(self):
        policy = select(np.full(30, .9), np.tile([False, True], 15))
        self.assertEqual(policy['status'], 'unsupported');self.assertIsNone(policy['winner'])
        result, accepted = evaluate([.99, .01], np.array([True, False]), None)
        self.assertFalse(accepted.any());self.assertEqual(result['unresolved'], 2)
        self.assertTrue(result['unresolved_is_not_no_change']);self.assertIsNone(result['precision'])

    def test_exact_precision_target_and_minimum_support(self):
        p = np.array([.8]*20+[.1]*30);y = np.array([True]*16+[False]*34)
        winner = select(p, y)['winner']
        self.assertEqual(winner['accepted'], 20);self.assertEqual(winner['counts']['precision'], .8)
        self.assertEqual(winner['cutoff'], .1)
        self.assertEqual(select(p[:19], y[:19])['status'], 'unsupported')
        y[15] = False;self.assertEqual(select(p, y)['status'], 'unsupported')

    def test_ties_are_not_split_and_maximum_eligible_coverage_wins(self):
        p = np.array([.9]*20+[.8]*10+[.2]*20);y = np.array([True]*30+[False]*20)
        winner = select(p, y)['winner'];self.assertEqual(winner['accepted'], 30)
        result, accepted = evaluate(p, y, winner['cutoff'])
        self.assertEqual(result['accepted'], 30);self.assertTrue(accepted[:30].all())
        self.assertFalse(accepted[30:].any())

    def test_reliability_endpoints_empty_bins_and_exact_edges(self):
        r = reliability([0., .1, .2, 1.], np.array([False, True, False, True]))
        self.assertEqual([b['count'] for b in r['bins']], [1, 1, 1, 0, 0, 0, 0, 0, 0, 1])
        self.assertIsNone(r['bins'][3]['absolute_gap']);self.assertEqual(r['brier'], (.81+.04)/4)
        self.assertAlmostEqual(r['ece'], 1.1/4)
        self.assertTrue(np.isfinite(r['log_loss']))

    def test_malformed_probabilities_and_labels_fail_closed(self):
        for p, y in [([], np.array([], dtype=bool)), ([np.nan], np.array([True])),
                     ([np.inf], np.array([True])), ([-.1], np.array([True])),
                     ([1.1], np.array([True])), ([.5], np.array([1])),
                     ([.5], np.array([True, False]))]:
            for function in [select, reliability]:
                with self.assertRaises(ValueError):function(p, y)


if __name__ == '__main__':unittest.main()
