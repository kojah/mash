import importlib.util
import itertools
import unittest

import numpy as np
from scipy.sparse import csr_matrix

from word_graph import Filter, assignments, communities, em_step, mixture, significance, source_mass


class WordGraphTests(unittest.TestCase):
    def test_hypergeometric_tail_matches_enumeration(self):
        counts = np.array([[1, 1], [1, 1], [1, 0], [0, 1], [0, 0], [0, 0]])
        graph = significance(counts, Filter.RAW)
        possible = list(itertools.combinations(range(6), 3))
        tail = sum(len(set(candidate) & {0, 1, 2}) >= 2 for candidate in possible) / len(possible)
        self.assertAlmostEqual(graph.probabilities[0], tail)
        self.assertEqual(graph.pairs.tolist(), [[0, 1]])
        np.testing.assert_array_equal(significance(counts * 100, Filter.RAW).probabilities, graph.probabilities)

    def test_bh_includes_zero_cooccurrence_tests_and_no_self_edges(self):
        counts = np.zeros((20, 20))
        counts[:2, :2] = 1  # p=1/190 passes raw but not 190-test BH family.
        counts[2:4, 2:] = np.eye(2, 18)
        raw, bh = [significance(counts, policy) for policy in Filter]
        self.assertEqual(len(bh.pairs), 190)
        self.assertEqual(int(raw.selected.sum()), 1)
        self.assertFalse(bh.selected.any())
        self.assertTrue(np.all(bh.probabilities[bh.counts == 0] == 1))
        self.assertTrue(np.all(bh.pairs[:, 0] < bh.pairs[:, 1]))

    def test_ubiquitous_word_does_not_make_significant_edges(self):
        graph = significance(np.array([[1, 1], [1, 1], [1, 0], [1, 0]]), Filter.RAW)
        self.assertEqual(graph.probabilities.tolist(), [1])
        self.assertFalse(graph.selected.any())

    def test_mass_preserves_isolate_denominator_and_abstains(self):
        values = np.array([[2, 1, 7], [0, 0, 1], [0, 0, 0]])
        np.testing.assert_allclose(source_mass(values, ((0, 1),)), [[.3], [0], [0]])
        self.assertEqual(assignments(np.array([[.3, .3, .1], [0, 0, 0]]), .1, 1), [[0], []])
        self.assertEqual(assignments(np.array([[.3, .14, .16]]), .1, 8), [[0, 2]])
        self.assertEqual(source_mass(values, ()).shape, (3, 0))

    def test_sparse_em_matches_dense_reference_on_fractional_counts(self):
        values = np.array([[.25, .75, 0], [0, .5, .5], [0, 0, 0]])
        theta = np.array([[.7, .3], [.2, .8], [0, 0]])
        phi = np.array([[.5, .4, .1], [.1, .2, .7]])
        probability = theta @ phi
        responsibilities = np.divide(theta[:, :, None] * phi[None, :, :], probability[:, None, :],
                                     out=np.zeros((3, 2, 3)), where=probability[:, None, :] > 0)
        expected = responsibilities * values[:, None, :]
        documents, words = expected.sum(2), expected.sum(0)
        np.testing.assert_allclose(expected.sum(1), values)
        np.divide(documents, documents.sum(1, keepdims=True), out=documents, where=documents.sum(1, keepdims=True) > 0)
        words /= words.sum(1, keepdims=True)
        actual_theta, actual_phi, likelihood = em_step(csr_matrix(values), theta, phi)
        np.testing.assert_allclose(actual_theta, documents)
        np.testing.assert_allclose(actual_phi, words)
        self.assertAlmostEqual(likelihood, float((values[values > 0] * np.log(probability[values > 0])).sum()))

    def test_mixture_monotonic_normalized_and_preserves_zero_rows(self):
        values = np.array([[1, 1, 0, 0, 8], [0, 0, 1, 1, 0], [.5, .5, .5, .5, 0], [0, 0, 0, 0, 1]])
        scores, phi, likelihood = mixture(values, ((0, 1), (2, 3)))
        self.assertEqual(len(likelihood), 51)
        self.assertTrue(np.all(np.diff(likelihood) >= -1e-10))
        np.testing.assert_allclose(scores.sum(1), [.2, 1, 1, 0])
        np.testing.assert_allclose(phi.sum(1), [1, 1])
        self.assertTrue(np.all(phi[:, 4] == 0))
        self.assertEqual(assignments(scores, .1, 8)[2], [0, 1])

    def test_malformed_counts_caps_and_overlapping_groups_rejected(self):
        for value in ([[np.nan]], [[-1]], np.zeros((0, 2)), [1, 2]):
            with self.assertRaises(ValueError):
                significance(value, Filter.RAW)
        with self.assertRaises(ValueError):
            significance(np.ones((1, 1025)), Filter.RAW)
        with self.assertRaises(ValueError):
            source_mass([[1, 1]], ((0,), (0,)))
        with self.assertRaises(ValueError):
            em_step(csr_matrix(np.ones((1001, 2))), np.ones((1001, 1000)), np.ones((1000, 2)))

    def test_collapsed_topic_stays_zero_without_repeated_smoothing(self):
        theta, phi, _ = em_step(csr_matrix([[.25, .75]]), np.array([[1., 0.]]), np.array([[.5, .5], [0., 0.]]))
        np.testing.assert_array_equal(theta, [[1, 0]])
        np.testing.assert_allclose(phi, [[.25, .75], [0, 0]])

    @unittest.skipUnless(importlib.util.find_spec('infomap'), 'optional graph research extra absent')
    def test_selection_requires_every_seed_and_uses_worst_case(self):
        from graph_questions import robust_selection
        def row(precision, recall):
            return {'metrics': {'tp': 21, 'fp': 9, 'precision': precision, 'recall': recall}}
        results = {'fragile/' + str(s): row(p, .9) for s, p in [(17, .8), (29, .69), (101, .8)]}
        self.assertIsNone(robust_selection(results))
        results.update({'stable/' + str(s): row(.75, .3) for s in (17, 29, 101)})
        results.update({'missing/17': row(.9, 1), 'missing/29': row(.9, 1)})
        self.assertEqual(robust_selection(results), 'stable')

    @unittest.skipUnless(importlib.util.find_spec('infomap'), 'optional graph research extra absent')
    def test_infomap_recovers_planted_components_with_canonical_repeat(self):
        values = np.zeros((40, 6))
        values[:4, :3] = 1
        values[4:8, 3:] = 1
        graph = significance(values, Filter.BH)
        first = communities(graph, 17)
        self.assertEqual(first[0], ((0, 1, 2), (3, 4, 5)))
        self.assertEqual(first, communities(graph, 17))
        self.assertEqual(communities(significance(np.zeros((4, 0)), Filter.BH), 17), ((), None))


if __name__ == '__main__':
    unittest.main()
