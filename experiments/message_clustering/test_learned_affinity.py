"""Graph projection and feature/model boundaries for learned turn affinity."""

import unittest

import numpy as np

from run_learned_affinity import Family, FeatureSet, Settings, fit, portable
from turn_affinity_features import NAMES, LinearScore, edge_cover, lexical, overlap


class LearnedAffinityTests(unittest.TestCase):
    def test_overlap_and_empty_lexical_input(self):
        self.assertEqual(overlap(frozenset({"v1"}), frozenset({"v2"})), (0.0, 1.0, 1.0))
        self.assertEqual(overlap(frozenset(), frozenset()), (0.0, 0.0, 0.0))
        np.testing.assert_array_equal(lexical(("", "")), np.zeros((2, 2)))

    def test_clique_cover_preserves_overlap_without_transitive_false_links(self):
        matrix = np.zeros((4, 4))
        for i, j in ((0, 1), (0, 2), (1, 2), (2, 3)):
            matrix[i, j] = matrix[j, i] = 0.9
        groups = edge_cover(matrix, 0.8)
        self.assertIn((0, 1, 2), groups)
        self.assertIn((2, 3), groups)
        covered = {(i, j) for group in groups for i in group for j in group if i < j}
        self.assertEqual(covered, {(0, 1), (0, 2), (1, 2), (2, 3)})
        self.assertEqual(groups, edge_cover(matrix, 0.8))
        matrix[0, 1] = np.nan
        with self.assertRaises(ValueError):
            edge_cover(matrix, 0.8)

    def test_fitted_scaling_uses_only_supplied_training_samples(self):
        values = np.zeros((4, len(NAMES)))
        values[:, 0] = [0.0, 0.1, 0.9, 1.0]
        settings = Settings(
            family=Family.LOGISTIC,
            features=FeatureSet.DENSE,
            cutoff=0.5,
            regularization=10.0,
        )
        model = fit(values, np.array([0, 0, 1, 1]), settings)
        means = np.asarray(model.scaler.mean_, dtype=np.float64)
        self.assertEqual(means.shape, (4,))
        self.assertAlmostEqual(means[0], 0.5)
        probabilities = model.predict(values)
        self.assertTrue(np.all(probabilities[:2] < 0.5))
        self.assertTrue(np.all(probabilities[2:] > 0.5))
        stored = portable(model)
        restored = LinearScore.model_validate_json(stored.model_dump_json())
        np.testing.assert_allclose(restored.predict(values), probabilities, atol=1e-12)


if __name__ == "__main__":
    unittest.main()
