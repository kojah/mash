from fractions import Fraction
import itertools
import math
import random
import unittest
from numeric_guidance import SCALE, key, nearest_even, ranked, retrieve, stability, fused


class NumericGuidance(unittest.TestCase):
    def test_exact_half_bins_even_odd_and_negative(self):
        # 10^12 contains2^12: these binary64 values yield exact half bins.
        for n in [-7, -5, -3, -1, 1, 3, 5, 7]:
            value = n / 8192.
            expected = round(Fraction.from_float(value) * SCALE)
            self.assertEqual(key(value), expected)
            self.assertEqual(nearest_even(Fraction(n, 2)), round(Fraction(n, 2)))

    def test_one_ulp_boundary_is_not_claimed_stable(self):
        middle = 1 / 8192.
        lo, hi = math.nextafter(middle, 0.), math.nextafter(middle, 1.)
        self.assertLess(key(lo), key(hi))
        result = stability(middle, math.ulp(middle))
        self.assertFalse(result.key_stable)
        self.assertTrue(result.eligibility_stable)
        self.assertFalse(result.stable)
        self.assertTrue(stability(.25, 0.).stable)

    def test_zero_and_tiny_positive_eligibility_separate_from_bins(self):
        tiny = math.nextafter(0., 1.)
        self.assertEqual(key(tiny), 0)
        self.assertEqual(key(-0.), 0)
        self.assertEqual([r.id for r in ranked([0., -0., tiny, -tiny], [3, 2, 9, 1])], [9])
        self.assertFalse(stability(tiny, tiny).eligibility_stable)
        self.assertTrue(stability(-0., 0.).stable)

    def test_close_nontransitive_epsilon_chain_has_total_order(self):
        # Adjacent differences below1e-12, endpoints above1e-12.
        values = [.5, .5 + .75e-12, .5 + 1.5e-12]
        ids = [19, 7, 11]
        expected = ranked(values, ids)
        for permutation in itertools.permutations(range(3)):
            self.assertEqual(ranked([values[i] for i in permutation], [ids[i] for i in permutation]), expected)

    def test_exact_fusion_ties_order_by_global_id(self):
        result = retrieve([.9, .8], [99, 3], [20, 10], [.8, .9], [20, 10])
        self.assertEqual(result.matches[0].score, result.matches[1].score)
        self.assertEqual([r.source.id for r in result.matches], [3, 99])
        self.assertEqual(fused(1, 2), fused(2, 1))
        self.assertEqual(fused(5, None), Fraction(1, 65))

    def test_sole_parent_preserves_quantized_direct_not_exact_float(self):
        values = [.4, math.nextafter(.4, 1.), .4, 0., -.1]
        ids = [99, 33, 10, 8, 7]
        for parent_score in [0., .6]:
            result = retrieve(values, ids, [123] * 5, [parent_score], [123])
            self.assertEqual([r.source.id for r in result.matches], [r.id for r in ranked(values, ids)])
        self.assertNotEqual(ranked(values, ids)[0].id, 33)

    def test_empty_and_full_positive_budget(self):
        self.assertEqual(retrieve([], [], [], [], []).matches, ())
        result = retrieve([.1] * 1000, list(range(1000)), [13] * 1000, [.5], [13])
        self.assertEqual(result.candidate_sources, 1000)
        self.assertEqual([r.source.id for r in result.matches], list(range(20)))

    def test_bad_inputs_rejected_before_filtering(self):
        for values, ids in [([float('nan')], [1]), ([float('inf')], [1]), ([True], [1]),
                            ([1], [1]), ([.3], [True]), ([.3], [-1]), ([.3, .2], [4, 4]), ([.3], [])]:
            with self.subTest(values=values, ids=ids), self.assertRaises(ValueError):
                ranked(values, ids)
        for owners in [[8], [], [True]]:
            with self.assertRaises(ValueError):retrieve([0.], [1], owners, [.5], [7])
        for rank in [0, -1, True, 1.]:
            with self.assertRaises(ValueError):fused(rank, None)
            with self.assertRaises(ValueError):fused(1, rank)
        with self.assertRaises(ValueError):stability(.5, -.1)

    def test_generated_separate_rational_oracle(self):
        rng = random.Random(20261002)
        for _ in range(200):
            n = rng.randrange(0, 80);p = rng.randrange(1, 12)
            ids = rng.sample(range(10000), n);parents = rng.sample(range(20000, 30000), p)
            values = [rng.choice([0., -.1, .5, math.nextafter(.5, 1.), rng.random()]) for _ in ids]
            scores = [rng.choice([0., .3, rng.random()]) for _ in parents]
            owners = [rng.choice(parents) for _ in ids]
            # Fraction.__round__ is an independent implementation of nearest-even.
            sources = sorted([(round(Fraction(v) * SCALE), i, v) for v, i in zip(values, ids) if v > 0], key=lambda r: (-r[0], r[1]))
            groups = sorted([(round(Fraction(v) * SCALE), i) for v, i in zip(scores, parents) if v > 0], key=lambda r: (-r[0], r[1]))
            ranks = {i: rank for rank, (_, i) in enumerate(groups, 1)};owner = dict(zip(ids, owners));expected = []
            for rank, (_, i, _) in enumerate(sources, 1):
                pr = ranks.get(owner[i]);weight = Fraction(1, 60 + rank) + (Fraction(1, 60 + pr) if pr else 0)
                expected.append((weight, i, rank, pr))
            expected.sort(key=lambda r: (-r[0], r[1]));actual = retrieve(values, ids, owners, scores, parents)
            self.assertEqual([(r.score, r.source.id, r.source.rank, r.parent.rank if r.parent else None) for r in actual.matches], expected[:20])
            self.assertEqual(actual.candidate_sources, len(sources))
            self.assertEqual(actual.positive_parents, len(groups))


if __name__ == '__main__':unittest.main()
